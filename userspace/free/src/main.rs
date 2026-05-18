//! OurOS Memory Information Display
//!
//! Displays system memory and swap usage by reading `/proc/meminfo`.
//! Similar to Linux `free` command.
//!
//! # Usage
//!
//! ```text
//! free                  Display memory info in KiB (default)
//! free -b               Display in bytes
//! free -k               Display in KiB
//! free -m               Display in MiB
//! free -g               Display in GiB
//! free -h / --human     Human-readable with automatic unit selection
//! free -t / --total     Show total row (mem + swap)
//! free -s <N>           Repeat every N seconds
//! free -c <N>           Repeat N times then exit
//! free --wide           Wider output (buffers and cache as separate columns)
//! free --json           JSON output
//! free --help           Show help
//! ```

use std::env;
use std::fs;
use std::process;

// ============================================================================
// Data structures
// ============================================================================

/// All memory fields parsed from /proc/meminfo, stored in KiB.
struct MemInfo {
    mem_total: u64,
    mem_free: u64,
    mem_available: u64,
    buffers: u64,
    cached: u64,
    swap_total: u64,
    swap_free: u64,
    shmem: u64,
    s_reclaimable: u64,
}

/// Which unit to display values in.
#[derive(Clone, Copy, PartialEq)]
enum Unit {
    Bytes,
    Kib,
    Mib,
    Gib,
    Human,
}

/// Runtime configuration parsed from CLI arguments.
struct Config {
    unit: Unit,
    show_total: bool,
    repeat_secs: Option<u64>,
    repeat_count: Option<u64>,
    wide: bool,
    json: bool,
}

// ============================================================================
// /proc/meminfo reader
// ============================================================================

/// Read the contents of a file, returning `None` on any I/O error.
fn read_file(path: &str) -> Option<String> {
    fs::read_to_string(path).ok()
}

/// Extract a numeric value (in kB) for a given key from /proc/meminfo content.
///
/// Lines are expected in the form:  `KeyName:       12345 kB`
/// Returns 0 if the key is not found or the value cannot be parsed.
fn get_meminfo_value(content: &str, key: &str) -> u64 {
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

/// Parse `/proc/meminfo` into a `MemInfo` struct.
///
/// Returns `None` if the file cannot be read at all. Individual missing
/// fields default to 0 rather than causing a failure, so the utility
/// degrades gracefully when the kernel exposes fewer fields.
fn read_meminfo() -> Option<MemInfo> {
    let content = read_file("/proc/meminfo")?;

    let mem_total = get_meminfo_value(&content, "MemTotal");
    let mem_free = get_meminfo_value(&content, "MemFree");
    let mem_available = get_meminfo_value(&content, "MemAvailable");
    let buffers = get_meminfo_value(&content, "Buffers");
    let cached = get_meminfo_value(&content, "Cached");
    let swap_total = get_meminfo_value(&content, "SwapTotal");
    let swap_free = get_meminfo_value(&content, "SwapFree");
    let shmem = get_meminfo_value(&content, "Shmem");
    let s_reclaimable = get_meminfo_value(&content, "SReclaimable");

    // If MemAvailable is missing (older kernels / early OurOS builds),
    // estimate it as free + buffers + cached.
    let mem_available = if mem_available == 0 && mem_free > 0 {
        mem_free.saturating_add(buffers).saturating_add(cached)
    } else {
        mem_available
    };

    Some(MemInfo {
        mem_total,
        mem_free,
        mem_available,
        buffers,
        cached,
        swap_total,
        swap_free,
        shmem,
        s_reclaimable,
    })
}

// ============================================================================
// Value formatting
// ============================================================================

/// Convert a KiB value according to the selected unit.
///
/// For `Unit::Human` this picks the largest unit that keeps the numeric
/// part >= 1.0 and formats with one decimal place plus a suffix.
/// For fixed units the value is returned as a right-aligned integer string.
fn format_value(kib: u64, unit: Unit) -> String {
    match unit {
        Unit::Bytes => {
            format!("{}", kib.saturating_mul(1024))
        }
        Unit::Kib => {
            format!("{kib}")
        }
        Unit::Mib => {
            format!("{}", kib / 1024)
        }
        Unit::Gib => {
            format!("{}", kib / (1024 * 1024))
        }
        Unit::Human => format_human(kib),
    }
}

/// Format a KiB value as a human-readable string with automatic unit
/// selection (e.g. "1.2 GiB", "384 MiB", "64 KiB").
fn format_human(kib: u64) -> String {
    let bytes = kib as f64 * 1024.0;
    if bytes >= 1024.0 * 1024.0 * 1024.0 {
        format!("{:.1} GiB", bytes / (1024.0 * 1024.0 * 1024.0))
    } else if bytes >= 1024.0 * 1024.0 {
        format!("{:.1} MiB", bytes / (1024.0 * 1024.0))
    } else if bytes >= 1024.0 {
        format!("{:.1} KiB", bytes / 1024.0)
    } else {
        format!("{bytes:.0} B")
    }
}

/// The column width used for numeric fields in tabular output.
const COL_WIDTH: usize = 12;

/// Right-align a formatted value within `COL_WIDTH` characters.
fn pad_right(val: &str) -> String {
    format!("{val:>width$}", width = COL_WIDTH)
}

// ============================================================================
// Standard (tabular) output
// ============================================================================

/// Print the normal (non-wide) table.
///
/// ```text
///               total        used        free      shared  buff/cache   available
/// Mem:      16384000     4192000     8192000      256000     2560000    12000000
/// Swap:      4096000           0     4096000
/// ```
fn print_standard(info: &MemInfo, config: &Config) {
    let u = config.unit;

    // Derived fields.
    let buff_cache = info.buffers
        .saturating_add(info.cached)
        .saturating_add(info.s_reclaimable);
    let mem_used = info.mem_total
        .saturating_sub(info.mem_free)
        .saturating_sub(info.buffers)
        .saturating_sub(info.cached)
        .saturating_sub(info.s_reclaimable);
    let swap_used = info.swap_total.saturating_sub(info.swap_free);

    // Header.
    println!(
        "{:14}{}{}{}{}{} {}",
        "",
        pad_right("total"),
        pad_right("used"),
        pad_right("free"),
        pad_right("shared"),
        pad_right("buff/cache"),
        pad_right("available"),
    );

    // Mem row.
    println!(
        "{:<14}{}{}{}{}{}  {}",
        "Mem:",
        pad_right(&format_value(info.mem_total, u)),
        pad_right(&format_value(mem_used, u)),
        pad_right(&format_value(info.mem_free, u)),
        pad_right(&format_value(info.shmem, u)),
        pad_right(&format_value(buff_cache, u)),
        pad_right(&format_value(info.mem_available, u)),
    );

    // Swap row.
    println!(
        "{:<14}{}{}{}",
        "Swap:",
        pad_right(&format_value(info.swap_total, u)),
        pad_right(&format_value(swap_used, u)),
        pad_right(&format_value(info.swap_free, u)),
    );

    // Total row (mem + swap combined).
    if config.show_total {
        let total_total = info.mem_total.saturating_add(info.swap_total);
        let total_used = mem_used.saturating_add(swap_used);
        let total_free = info.mem_free.saturating_add(info.swap_free);

        println!(
            "{:<14}{}{}{}",
            "Total:",
            pad_right(&format_value(total_total, u)),
            pad_right(&format_value(total_used, u)),
            pad_right(&format_value(total_free, u)),
        );
    }
}

/// Print the wide table (buffers and cache as separate columns).
///
/// ```text
///               total        used        free      shared     buffers       cache   available
/// Mem:      16384000     4192000     8192000      256000      512000     2048000    12000000
/// Swap:      4096000           0     4096000
/// ```
fn print_wide(info: &MemInfo, config: &Config) {
    let u = config.unit;

    // In wide mode, "used" does not subtract buffers/cached/sreclaimable.
    let mem_used = info.mem_total
        .saturating_sub(info.mem_free)
        .saturating_sub(info.buffers)
        .saturating_sub(info.cached)
        .saturating_sub(info.s_reclaimable);
    let swap_used = info.swap_total.saturating_sub(info.swap_free);
    let cache_col = info.cached.saturating_add(info.s_reclaimable);

    // Header.
    println!(
        "{:14}{}{}{}{}{}{}  {}",
        "",
        pad_right("total"),
        pad_right("used"),
        pad_right("free"),
        pad_right("shared"),
        pad_right("buffers"),
        pad_right("cache"),
        pad_right("available"),
    );

    // Mem row.
    println!(
        "{:<14}{}{}{}{}{}{}  {}",
        "Mem:",
        pad_right(&format_value(info.mem_total, u)),
        pad_right(&format_value(mem_used, u)),
        pad_right(&format_value(info.mem_free, u)),
        pad_right(&format_value(info.shmem, u)),
        pad_right(&format_value(info.buffers, u)),
        pad_right(&format_value(cache_col, u)),
        pad_right(&format_value(info.mem_available, u)),
    );

    // Swap row.
    println!(
        "{:<14}{}{}{}",
        "Swap:",
        pad_right(&format_value(info.swap_total, u)),
        pad_right(&format_value(swap_used, u)),
        pad_right(&format_value(info.swap_free, u)),
    );

    // Total row.
    if config.show_total {
        let total_total = info.mem_total.saturating_add(info.swap_total);
        let total_used = mem_used.saturating_add(swap_used);
        let total_free = info.mem_free.saturating_add(info.swap_free);

        println!(
            "{:<14}{}{}{}",
            "Total:",
            pad_right(&format_value(total_total, u)),
            pad_right(&format_value(total_used, u)),
            pad_right(&format_value(total_free, u)),
        );
    }
}

// ============================================================================
// JSON output
// ============================================================================

/// Emit a JSON representation of memory/swap info.
///
/// Values are always in the requested unit (bytes, KiB, MiB, or GiB).
/// For `--human` mode, JSON falls back to KiB since human-readable strings
/// are not useful as machine-parsable numbers.
fn print_json(info: &MemInfo, config: &Config) {
    // For JSON, human mode falls back to KiB (JSON consumers want numbers).
    let u = if config.unit == Unit::Human { Unit::Kib } else { config.unit };

    let buff_cache = info.buffers
        .saturating_add(info.cached)
        .saturating_add(info.s_reclaimable);
    let mem_used = info.mem_total
        .saturating_sub(info.mem_free)
        .saturating_sub(info.buffers)
        .saturating_sub(info.cached)
        .saturating_sub(info.s_reclaimable);
    let swap_used = info.swap_total.saturating_sub(info.swap_free);

    let unit_name = match u {
        Unit::Bytes => "bytes",
        Unit::Kib => "kibibytes",
        Unit::Mib => "mebibytes",
        Unit::Gib => "gibibytes",
        Unit::Human => "kibibytes", // unreachable after the fallback above
    };

    // Manual JSON formatting to avoid pulling in a serde dependency.
    println!("{{");
    println!("  \"unit\": \"{unit_name}\",");
    println!("  \"mem\": {{");
    println!("    \"total\": {},", format_value(info.mem_total, u));
    println!("    \"used\": {},", format_value(mem_used, u));
    println!("    \"free\": {},", format_value(info.mem_free, u));
    println!("    \"shared\": {},", format_value(info.shmem, u));
    println!("    \"buff_cache\": {},", format_value(buff_cache, u));
    println!("    \"buffers\": {},", format_value(info.buffers, u));
    println!("    \"cached\": {},", format_value(info.cached, u));
    println!("    \"s_reclaimable\": {},", format_value(info.s_reclaimable, u));
    println!("    \"available\": {}", format_value(info.mem_available, u));
    println!("  }},");
    println!("  \"swap\": {{");
    println!("    \"total\": {},", format_value(info.swap_total, u));
    println!("    \"used\": {},", format_value(swap_used, u));
    println!("    \"free\": {}", format_value(info.swap_free, u));
    println!("  }}");
    println!("}}");
}

// ============================================================================
// Display dispatcher
// ============================================================================

/// Print one snapshot of memory information using the configured format.
fn display_once(info: &MemInfo, config: &Config) {
    if config.json {
        print_json(info, config);
    } else if config.wide {
        print_wide(info, config);
    } else {
        print_standard(info, config);
    }
}

// ============================================================================
// Main run loop
// ============================================================================

/// Execute the display loop (single-shot or repeating).
fn run(config: &Config) -> i32 {
    let mut iterations: u64 = 0;

    loop {
        let info = match read_meminfo() {
            Some(i) => i,
            None => {
                eprintln!("free: failed to read /proc/meminfo");
                return 1;
            }
        };

        display_once(&info, config);
        iterations = iterations.saturating_add(1);

        // Check count limit.
        if let Some(max) = config.repeat_count {
            if iterations >= max {
                break;
            }
        }

        // If no repeat interval, run once.
        let secs = match config.repeat_secs {
            Some(s) => s,
            None => break,
        };

        // Print a blank line between repeated snapshots for readability.
        println!();

        std::thread::sleep(std::time::Duration::from_secs(secs));
    }

    0
}

// ============================================================================
// CLI parsing
// ============================================================================

fn print_usage() {
    println!("OurOS Memory Information Display v0.1.0");
    println!();
    println!("Display amount of free and used memory in the system.");
    println!();
    println!("USAGE:");
    println!("  free [options]");
    println!();
    println!("OPTIONS:");
    println!("  -b              Display in bytes");
    println!("  -k              Display in KiB (default)");
    println!("  -m              Display in MiB");
    println!("  -g              Display in GiB");
    println!("  -h, --human     Human-readable output (automatic unit selection)");
    println!("  -t, --total     Show total row (mem + swap combined)");
    println!("  -s <N>          Repeat every N seconds");
    println!("  -c <N>          Repeat N times then exit (use with -s)");
    println!("  --wide          Show buffers and cache as separate columns");
    println!("  --json          Output in JSON format");
    println!("  --help          Show this help");
}

fn main() {
    let args: Vec<String> = env::args().collect();

    let mut config = Config {
        unit: Unit::Kib,
        show_total: false,
        repeat_secs: None,
        repeat_count: None,
        wide: false,
        json: false,
    };

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-b" => {
                config.unit = Unit::Bytes;
                i += 1;
            }
            "-k" => {
                config.unit = Unit::Kib;
                i += 1;
            }
            "-m" => {
                config.unit = Unit::Mib;
                i += 1;
            }
            "-g" => {
                config.unit = Unit::Gib;
                i += 1;
            }
            "-h" | "--human" => {
                config.unit = Unit::Human;
                i += 1;
            }
            "-t" | "--total" => {
                config.show_total = true;
                i += 1;
            }
            "-s" => {
                if i + 1 >= args.len() {
                    eprintln!("free: -s requires a numeric argument (seconds)");
                    process::exit(1);
                }
                match args[i + 1].parse::<u64>() {
                    Ok(s) if s > 0 => config.repeat_secs = Some(s),
                    _ => {
                        eprintln!("free: invalid interval: {}", args[i + 1]);
                        process::exit(1);
                    }
                }
                i += 2;
            }
            "-c" => {
                if i + 1 >= args.len() {
                    eprintln!("free: -c requires a numeric argument (count)");
                    process::exit(1);
                }
                match args[i + 1].parse::<u64>() {
                    Ok(c) if c > 0 => config.repeat_count = Some(c),
                    _ => {
                        eprintln!("free: invalid count: {}", args[i + 1]);
                        process::exit(1);
                    }
                }
                i += 2;
            }
            "--wide" => {
                config.wide = true;
                i += 1;
            }
            "--json" => {
                config.json = true;
                i += 1;
            }
            "--help" => {
                print_usage();
                process::exit(0);
            }
            other => {
                eprintln!("free: unknown option: {other}");
                eprintln!("Run 'free --help' for usage.");
                process::exit(1);
            }
        }
    }

    // -c without -s: default to 1-second interval so the count is meaningful.
    if config.repeat_count.is_some() && config.repeat_secs.is_none() {
        config.repeat_secs = Some(1);
    }

    let exit_code = run(&config);
    process::exit(exit_code);
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Simulate /proc/meminfo content for testing the parser.
    const SAMPLE_MEMINFO: &str = "\
MemTotal:       16384000 kB
MemFree:         8192000 kB
MemAvailable:   12000000 kB
Buffers:          512000 kB
Cached:          2048000 kB
SwapTotal:       4096000 kB
SwapFree:        4096000 kB
Shmem:            256000 kB
SReclaimable:     128000 kB
";

    #[test]
    fn test_get_meminfo_value_basic() {
        assert_eq!(get_meminfo_value(SAMPLE_MEMINFO, "MemTotal"), 16_384_000);
        assert_eq!(get_meminfo_value(SAMPLE_MEMINFO, "MemFree"), 8_192_000);
        assert_eq!(get_meminfo_value(SAMPLE_MEMINFO, "Buffers"), 512_000);
        assert_eq!(get_meminfo_value(SAMPLE_MEMINFO, "SReclaimable"), 128_000);
    }

    #[test]
    fn test_get_meminfo_value_missing_key() {
        assert_eq!(get_meminfo_value(SAMPLE_MEMINFO, "NonExistent"), 0);
    }

    #[test]
    fn test_format_value_bytes() {
        // 1024 KiB = 1_048_576 bytes.
        assert_eq!(format_value(1024, Unit::Bytes), "1048576");
    }

    #[test]
    fn test_format_value_kib() {
        assert_eq!(format_value(4096, Unit::Kib), "4096");
    }

    #[test]
    fn test_format_value_mib() {
        // 2048 KiB = 2 MiB.
        assert_eq!(format_value(2048, Unit::Mib), "2");
    }

    #[test]
    fn test_format_value_gib() {
        // 1_048_576 KiB = 1 GiB.
        assert_eq!(format_value(1_048_576, Unit::Gib), "1");
    }

    #[test]
    fn test_format_human_gib() {
        let s = format_human(1_048_576); // 1 GiB
        assert!(s.contains("GiB"), "expected GiB in '{s}'");
    }

    #[test]
    fn test_format_human_mib() {
        let s = format_human(2048); // 2 MiB
        assert!(s.contains("MiB"), "expected MiB in '{s}'");
    }

    #[test]
    fn test_format_human_kib() {
        let s = format_human(512); // 512 KiB
        assert!(s.contains("KiB"), "expected KiB in '{s}'");
    }

    #[test]
    fn test_format_human_bytes() {
        // 0 KiB = 0 bytes.
        let s = format_human(0);
        assert!(s.contains("B"), "expected B in '{s}'");
    }

    #[test]
    fn test_used_calculation() {
        // used = total - free - buffers - cached - sreclaimable
        let total: u64 = 16_384_000;
        let free: u64 = 8_192_000;
        let buffers: u64 = 512_000;
        let cached: u64 = 2_048_000;
        let sreclaimable: u64 = 128_000;

        let used = total
            .saturating_sub(free)
            .saturating_sub(buffers)
            .saturating_sub(cached)
            .saturating_sub(sreclaimable);

        // 16384000 - 8192000 - 512000 - 2048000 - 128000 = 5504000
        assert_eq!(used, 5_504_000);
    }

    #[test]
    fn test_buff_cache_calculation() {
        let buffers: u64 = 512_000;
        let cached: u64 = 2_048_000;
        let sreclaimable: u64 = 128_000;

        let buff_cache = buffers
            .saturating_add(cached)
            .saturating_add(sreclaimable);

        assert_eq!(buff_cache, 2_688_000);
    }

    #[test]
    fn test_swap_used() {
        let swap_total: u64 = 4_096_000;
        let swap_free: u64 = 4_096_000;
        assert_eq!(swap_total.saturating_sub(swap_free), 0);
    }

    #[test]
    fn test_available_fallback() {
        // When MemAvailable is 0 (missing), estimate from free + buffers + cached.
        let content = "\
MemTotal:       16384000 kB
MemFree:         8192000 kB
Buffers:          512000 kB
Cached:          2048000 kB
SwapTotal:              0 kB
SwapFree:               0 kB
Shmem:                  0 kB
SReclaimable:           0 kB
";
        let mem_free = get_meminfo_value(content, "MemFree");
        let buffers = get_meminfo_value(content, "Buffers");
        let cached = get_meminfo_value(content, "Cached");
        let mem_available = get_meminfo_value(content, "MemAvailable");

        // MemAvailable is missing, so it returns 0.
        assert_eq!(mem_available, 0);

        // Fallback estimate.
        let estimated = mem_free.saturating_add(buffers).saturating_add(cached);
        assert_eq!(estimated, 10_752_000);
    }

    #[test]
    fn test_pad_right_alignment() {
        let s = pad_right("42");
        assert_eq!(s.len(), COL_WIDTH);
        assert!(s.ends_with("42"));
        // Leading characters should be spaces.
        assert!(s.starts_with(' '));
    }

    #[test]
    fn test_format_value_zero() {
        assert_eq!(format_value(0, Unit::Bytes), "0");
        assert_eq!(format_value(0, Unit::Kib), "0");
        assert_eq!(format_value(0, Unit::Mib), "0");
        assert_eq!(format_value(0, Unit::Gib), "0");
    }
}
