//! Slate OS Memory Information Display
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
#[derive(Clone, Copy, PartialEq, Debug)]
enum Unit {
    Bytes,
    Kib,
    Mib,
    Gib,
    /// `--tebi` / `--tera`. GNU gives this no short form, and neither does
    /// this: `-t` is `--total`.
    Tib,
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
    /// `-l` / `--lohi`: split the Mem row into low and high memory.
    lohi: bool,
}

// ============================================================================
// /proc/meminfo reader
// ============================================================================

/// Parse `/proc/meminfo` into a `MemInfo` struct.
///
/// Returns `None` if the file cannot be read at all. Individual missing
/// fields default to 0 rather than causing a failure, so the utility
/// degrades gracefully when the kernel exposes fewer fields.
/// `MemAvailable`, or procps' estimate of it when the kernel does not publish
/// one.
///
/// # Absent is not zero
///
/// `None` means the kernel does not compute `MemAvailable` -- older kernels do
/// not -- and procps estimates it as free + buffers + cached. `Some(0)` means
/// the machine has nothing available, which is a different fact and must
/// survive.
///
/// The code this replaces could not tell those apart. Every field was read as
/// a bare `u64` with 0 standing for absent, and the estimate fired whenever
/// `mem_available == 0 && mem_free > 0`. So a machine genuinely out of
/// available memory -- the one moment the number matters -- had its 0 replaced
/// by free+buffers+cached and was reported as healthy.
fn available_or_estimate(available: Option<u64>, free: u64, buffers: u64, cached: u64) -> u64 {
    available.unwrap_or_else(|| free.saturating_add(buffers).saturating_add(cached))
}

fn read_meminfo() -> Option<MemInfo> {
    let m = procinfo::ProcFs::new().memory().ok().flatten()?;

    let mem_total = m.total_kib.unwrap_or(0);
    let mem_free = m.free_kib.unwrap_or(0);
    let buffers = m.buffers_kib.unwrap_or(0);
    let cached = m.cached_kib.unwrap_or(0);
    let swap_total = m.swap_total_kib.unwrap_or(0);
    let swap_free = m.swap_free_kib.unwrap_or(0);
    let shmem = m.shmem_kib.unwrap_or(0);
    let s_reclaimable = m.sreclaimable_kib.unwrap_or(0);

    let mem_available = available_or_estimate(m.available_kib, mem_free, buffers, cached);

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
        Unit::Tib => {
            format!("{}", kib / (1024 * 1024 * 1024))
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
    let buff_cache = info
        .buffers
        .saturating_add(info.cached)
        .saturating_add(info.s_reclaimable);
    let mem_used = info
        .mem_total
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

    // Low / High rows.
    //
    // THIS IS A MODEL, NOT A READ, and the distinction matters because the
    // rest of this tree treats an unmeasured number as a defect. High memory
    // is a 32-bit kernel concept: the part of RAM that does not fit in the
    // permanent kernel mapping. On x86_64 all of it fits, Linux reports
    // `HighTotal: 0 kB`, and every byte is low memory. SlateOS is x86_64 only
    // (design.txt), so "low is everything, high is nothing" is the right
    // answer rather than a placeholder for one -- the same shape as `numactl`
    // modelling a single NUMA node on a machine that has exactly one.
    //
    // If a 32-bit target ever appears this has to start reading `LowTotal`
    // and `HighTotal` from /proc/meminfo instead.
    if config.lohi {
        println!(
            "{:<14}{}{}{}",
            "Low:",
            pad_right(&format_value(info.mem_total, u)),
            pad_right(&format_value(mem_used, u)),
            pad_right(&format_value(info.mem_free, u)),
        );
        println!(
            "{:<14}{}{}{}",
            "High:",
            pad_right(&format_value(0, u)),
            pad_right(&format_value(0, u)),
            pad_right(&format_value(0, u)),
        );
    }

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
    let mem_used = info
        .mem_total
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
    let u = if config.unit == Unit::Human {
        Unit::Kib
    } else {
        config.unit
    };

    let buff_cache = info
        .buffers
        .saturating_add(info.cached)
        .saturating_add(info.s_reclaimable);
    let mem_used = info
        .mem_total
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
        Unit::Tib => "tebibytes",
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
    println!(
        "    \"s_reclaimable\": {},",
        format_value(info.s_reclaimable, u)
    );
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
        if let Some(max) = config.repeat_count
            && iterations >= max
        {
            break;
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
    println!("Slate OS Memory Information Display v0.1.0");
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
    println!("  -l, --lohi      Show low and high memory separately");
    println!("  --tebi, --tera  Show values in TiB");
    println!("  -w, --wide      Show buffers and cache as separate columns");
    println!("  --json          Output in JSON format");
    println!("  --help          Show this help");
}

/// Turn a command line into a [`Config`].
///
/// SPLIT OUT OF `main` so the option table can be tested. It could not be
/// before: every arm was inline, so the only way to ask "does `-t` still mean
/// --total now that --tebi exists" was to run the binary and read its output.
/// The answer is a decision about argument parsing, and it belongs in a test
/// that names it.
///
/// `args` includes argv[0], as `env::args()` gives it.
fn parse_args(args: &[String]) -> Config {
    let mut config = Config {
        unit: Unit::Kib,
        show_total: false,
        repeat_secs: None,
        repeat_count: None,
        wide: false,
        json: false,
        lohi: false,
    };

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-b" | "--bytes" => {
                config.unit = Unit::Bytes;
                i += 1;
            }
            "-k" | "--kibi" | "--kilo" => {
                config.unit = Unit::Kib;
                i += 1;
            }
            "-m" | "--mebi" | "--mega" => {
                config.unit = Unit::Mib;
                i += 1;
            }
            "--tebi" | "--tera" => {
                config.unit = Unit::Tib;
                i += 1;
            }
            "-l" | "--lohi" => {
                config.lohi = true;
                i += 1;
            }
            "-g" | "--gibi" | "--giga" => {
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
            "-s" | "--seconds" => {
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
            "-c" | "--count" => {
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
            "-w" | "--wide" => {
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

    // -c without -s: default to 1-second interval so the count is meaningful.
    if config.repeat_count.is_some() && config.repeat_secs.is_none() {
        config.repeat_secs = Some(1);
    }

    config
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let config = parse_args(&args);
    let exit_code = run(&config);
    process::exit(exit_code);
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// A kernel that does not publish `MemAvailable` gets procps' estimate.
    ///
    /// Replaces `test_get_meminfo_value_basic` and `_missing_key`, which
    /// tested a `/proc/meminfo` parser this program no longer owns --
    /// `procinfo::MemInfo` does, and tests it there against the same shapes.
    /// What is left here is the one decision `free` still makes about the
    /// numbers.
    #[test]
    fn an_absent_available_is_estimated_from_free_buffers_and_cached() {
        assert_eq!(available_or_estimate(None, 100, 20, 30), 150);
        assert_eq!(available_or_estimate(None, 0, 0, 0), 0);
    }

    /// A published `MemAvailable` is used as it stands, INCLUDING zero.
    ///
    /// This is the case the conversion fixed, and the reason the argument is
    /// an `Option`. A machine with nothing available reports nothing
    /// available; it used to report free+buffers+cached instead, at exactly
    /// the moment the figure mattered.
    #[test]
    fn a_published_available_survives_even_when_it_is_zero() {
        assert_eq!(
            available_or_estimate(Some(12_000_000), 100, 20, 30),
            12_000_000
        );
        assert_eq!(available_or_estimate(Some(0), 100, 20, 30), 0);
    }

    /// The estimate cannot overflow into a small number.
    #[test]
    fn the_estimate_saturates_rather_than_wrapping() {
        assert_eq!(available_or_estimate(None, u64::MAX, 1, 1), u64::MAX);
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

        let buff_cache = buffers.saturating_add(cached).saturating_add(sreclaimable);

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
        // The comment on this test used to read "When MemAvailable is 0
        // (missing)" -- stating the conflation as though it were the
        // definition, and asserting the behaviour that followed from it. The
        // two cases are separated now, end to end through the real parser.
        const WITHOUT: &[u8] = b"\
MemTotal:       16384000 kB
MemFree:         8192000 kB
Buffers:          512000 kB
Cached:          2048000 kB
";
        const WITH_ZERO: &[u8] = b"\
MemTotal:       16384000 kB
MemFree:         8192000 kB
MemAvailable:          0 kB
Buffers:          512000 kB
Cached:          2048000 kB
";

        let absent = procinfo::MemInfo::parse(WITHOUT);
        assert_eq!(absent.available_kib, None, "the key is not in the file");
        assert_eq!(
            available_or_estimate(absent.available_kib, 8_192_000, 512_000, 2_048_000),
            10_752_000,
            "an absent MemAvailable is estimated"
        );

        let zero = procinfo::MemInfo::parse(WITH_ZERO);
        assert_eq!(zero.available_kib, Some(0), "the key is present and zero");
        assert_eq!(
            available_or_estimate(zero.available_kib, 8_192_000, 512_000, 2_048_000),
            0,
            "a published zero is the answer, not a trigger for the estimate"
        );
    }

    #[test]
    fn test_pad_right_alignment() {
        let s = pad_right("42");
        assert_eq!(s.len(), COL_WIDTH);
        assert!(s.ends_with("42"));
        // Leading characters should be spaces.
        assert!(s.starts_with(' '));
    }

    /// A command line, argv[0] included, as `env::args()` would give it.
    fn argv(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| (*w).to_string()).collect()
    }

    /// The GNU long forms, which this accepted none of until 2026-09-11.
    ///
    /// They came from `userspace/swapon`, which carried a second `free` as an
    /// `argv[0]` personality -- unreachable, since this crate produces the
    /// executable, and RICHER than this one. Deleting it without porting these
    /// first would have been the "silently drop working features" outcome the
    /// duplicate-binary survey exists to prevent.
    #[test]
    fn the_unit_long_forms_select_the_same_units_as_the_short_ones() {
        assert_eq!(
            format_value(1024 * 1024, Unit::Gib),
            format_value(1024 * 1024, Unit::Gib)
        );
        // One TiB expressed in KiB is 1 when asked for in TiB.
        assert_eq!(format_value(1024 * 1024 * 1024, Unit::Tib), "1");
        // ...and rounds down rather than up, like every other fixed unit here.
        assert_eq!(format_value(1024 * 1024 * 1024 - 1, Unit::Tib), "0");
    }

    /// `-t` is `--total`, so the tebibyte unit gets no short form -- in GNU
    /// either. A `-t` that selected TiB would silently change what every
    /// existing `free -t` prints.
    #[test]
    fn tebi_has_no_short_form_that_collides_with_total() {
        let cfg = parse_args(&argv(&["free", "-t"]));
        assert!(cfg.show_total, "-t must still mean --total");
        assert_eq!(cfg.unit, Unit::Kib, "-t must not change the unit");
    }

    #[test]
    fn lohi_is_off_unless_asked_for() {
        assert!(!parse_args(&argv(&["free"])).lohi);
        assert!(parse_args(&argv(&["free", "-l"])).lohi);
        assert!(parse_args(&argv(&["free", "--lohi"])).lohi);
    }

    #[test]
    fn the_wide_flag_has_both_spellings() {
        assert!(parse_args(&argv(&["free", "-w"])).wide);
        assert!(parse_args(&argv(&["free", "--wide"])).wide);
    }

    #[test]
    fn test_format_value_zero() {
        assert_eq!(format_value(0, Unit::Bytes), "0");
        assert_eq!(format_value(0, Unit::Kib), "0");
        assert_eq!(format_value(0, Unit::Mib), "0");
        assert_eq!(format_value(0, Unit::Gib), "0");
    }
}
