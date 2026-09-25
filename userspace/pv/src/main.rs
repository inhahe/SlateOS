//! Slate OS pv -- monitor data flowing through a pipe (pipe viewer).
//!
//! This was a multi-personality binary chosen by argv\[0\], and is `pv` alone
//! now. `truncate` and `shred` were personalities here too, which no link
//! ever reached; each is `userspace/coreutils`'s own bin since 2026-09-25, a
//! port of GNU's checked against it -- `shred`'s writing the very bytes GNU's
//! does from the same `--random-source`, where this one wrote xorshift output
//! under a help text that promised `/dev/urandom` -- and `coreutils` is the
//! one home for such a name, the duplicate going (design-decisions.md §1005).
//! With one mode left, the dispatch went too.
//!
//! # Example
//!
//! ```text
//! pv -s 100M bigfile.iso | gzip > bigfile.iso.gz
//! ```

use quoting::quoteaf_os;
use std::env;
use std::fs::File;
use std::io::{self, Read, Write};
use std::process;
use std::time::Instant;

// ============================================================================
// Constants
// ============================================================================

/// Default read buffer size for pv (128 KiB).
const DEFAULT_BUFFER_SIZE: usize = 128 * 1024;

/// Progress update interval in milliseconds.
const PROGRESS_INTERVAL_MS: u128 = 100;

/// Width allocated for the progress bar (excluding brackets).
const DEFAULT_BAR_WIDTH: usize = 25;

// ============================================================================
// Size parsing and formatting helpers
// ============================================================================

/// Parse a human-readable size string with optional K/M/G/T/P/E suffix.
///
/// Returns bytes. The suffix multiplier is powers of 1024.
fn parse_size(s: &str) -> Result<u64, String> {
    if s.is_empty() {
        return Err("empty size value".into());
    }

    let s = s.trim();
    if s.is_empty() {
        return Err("empty size value".into());
    }

    // Find where the numeric part ends and the suffix begins.
    let (num_str, suffix) = split_number_suffix(s);

    let base: u64 = num_str
        .parse()
        .map_err(|e| format!("invalid number {}: {e}", quoteaf_os(num_str)))?;

    let multiplier = suffix_multiplier(suffix)?;

    base.checked_mul(multiplier)
        .ok_or_else(|| format!("size overflow: {s}"))
}

/// Split a string like "100M" into ("100", "M").
fn split_number_suffix(s: &str) -> (&str, &str) {
    // Walk backwards from the end to find where digits stop.
    let suffix_start = s
        .rfind(|c: char| c.is_ascii_digit())
        .map(|i| i + 1)
        .unwrap_or(0);
    (&s[..suffix_start], &s[suffix_start..])
}

/// Map a suffix string to its multiplier (powers of 1024).
fn suffix_multiplier(suffix: &str) -> Result<u64, String> {
    match suffix {
        "" | "B" => Ok(1),
        "K" | "KB" | "k" => Ok(1024),
        "M" | "MB" => Ok(1024 * 1024),
        "G" | "GB" => Ok(1024 * 1024 * 1024),
        "T" | "TB" => Ok(1024u64 * 1024 * 1024 * 1024),
        "P" | "PB" => Ok(1024u64 * 1024 * 1024 * 1024 * 1024),
        "E" | "EB" => Ok(1024u64 * 1024 * 1024 * 1024 * 1024 * 1024),
        _ => Err(format!("unknown size suffix {}", quoteaf_os(suffix))),
    }
}

/// Format a byte count as a human-readable string (e.g. "12.3MB").
fn format_size(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];

    if bytes == 0 {
        return "0B".into();
    }

    let mut value = bytes as f64;
    let mut unit_idx = 0;

    while value >= 1024.0 && unit_idx + 1 < UNITS.len() {
        value /= 1024.0;
        unit_idx += 1;
    }

    if unit_idx == 0 {
        format!("{bytes}B")
    } else if value >= 100.0 {
        format!("{:.0}{}", value, UNITS[unit_idx])
    } else if value >= 10.0 {
        format!("{:.1}{}", value, UNITS[unit_idx])
    } else {
        format!("{:.2}{}", value, UNITS[unit_idx])
    }
}

/// Format elapsed seconds as H:MM:SS.
fn format_time(total_secs: u64) -> String {
    let hours = total_secs / 3600;
    let mins = (total_secs % 3600) / 60;
    let secs = total_secs % 60;
    format!("{hours}:{mins:02}:{secs:02}")
}

/// Calculate throughput in bytes per second.
fn calc_throughput(bytes: u64, elapsed_ms: u128) -> f64 {
    if elapsed_ms == 0 {
        return 0.0;
    }
    (bytes as f64) / (elapsed_ms as f64 / 1000.0)
}

/// Calculate ETA in seconds given current throughput and remaining bytes.
fn calc_eta(remaining: u64, throughput_bps: f64) -> Option<u64> {
    if throughput_bps <= 0.0 || remaining == 0 {
        return Some(0);
    }
    let secs = (remaining as f64) / throughput_bps;
    if secs > 359_999.0 {
        // More than 99:59:59 -- don't display
        None
    } else {
        Some(secs as u64)
    }
}

// ============================================================================
// Progress bar rendering
// ============================================================================

/// Render a progress bar string for the given parameters.
///
/// Format: `[====>                    ] 45% 12.3MB 4.56MB/s ETA 0:00:15`
fn render_progress_bar(
    transferred: u64,
    total: Option<u64>,
    elapsed_ms: u128,
    bar_width: usize,
    name: Option<&str>,
) -> String {
    let throughput = calc_throughput(transferred, elapsed_ms);
    let throughput_str = format!("{}/s", format_size(throughput as u64));
    let transferred_str = format_size(transferred);
    let elapsed_secs = (elapsed_ms / 1000) as u64;

    let mut parts = Vec::new();

    if let Some(n) = name {
        parts.push(format!("{n}: "));
    }

    if let Some(tot) = total {
        let pct = if tot == 0 {
            100.0
        } else {
            (transferred as f64 / tot as f64) * 100.0
        };
        let pct_clamped = pct.min(100.0);

        // Build the bar
        let filled = ((pct_clamped / 100.0) * bar_width as f64) as usize;
        let filled = filled.min(bar_width);
        let empty = bar_width.saturating_sub(filled);

        let bar = if filled > 0 && filled < bar_width {
            format!(
                "[{}{}>{}]",
                "=".repeat(filled.saturating_sub(1)),
                "",
                " ".repeat(empty)
            )
        } else if filled >= bar_width {
            format!("[{}]", "=".repeat(bar_width))
        } else {
            format!("[>{}]", " ".repeat(bar_width.saturating_sub(1)))
        };

        parts.push(bar);
        parts.push(format!(" {:.0}%", pct_clamped));

        let remaining = tot.saturating_sub(transferred);
        if let Some(eta) = calc_eta(remaining, throughput) {
            parts.push(format!(
                " {} {} ETA {}",
                transferred_str,
                throughput_str,
                format_time(eta)
            ));
        } else {
            parts.push(format!(" {} {}", transferred_str, throughput_str));
        }
    } else {
        // No total size known -- just show transferred, throughput, elapsed
        parts.push(format!(
            "{} {} {}",
            transferred_str,
            throughput_str,
            format_time(elapsed_secs)
        ));
    }

    parts.concat()
}

// ============================================================================
// pv mode
// ============================================================================

/// Configuration for pv mode, parsed from command-line arguments.
struct PvConfig {
    expected_size: Option<u64>,
    numeric: bool,
    quiet: bool,
    line_mode: bool,
    rate_limit: Option<u64>,
    buffer_size: usize,
    name: Option<String>,
    wait_first_byte: bool,
    force: bool,
    cursor: bool,
    files: Vec<String>,
}

impl Default for PvConfig {
    fn default() -> Self {
        Self {
            expected_size: None,
            numeric: false,
            quiet: false,
            line_mode: false,
            rate_limit: None,
            buffer_size: DEFAULT_BUFFER_SIZE,
            name: None,
            wait_first_byte: false,
            force: false,
            cursor: false,
            files: Vec::new(),
        }
    }
}

fn parse_pv_args(args: &[String]) -> Result<PvConfig, String> {
    let mut cfg = PvConfig::default();
    let mut i = 0;

    while i < args.len() {
        let arg = &args[i];
        if arg == "--help" || arg == "-h" {
            print_pv_usage();
            process::exit(0);
        } else if arg == "-n" || arg == "--numeric" {
            cfg.numeric = true;
        } else if arg == "-q" || arg == "--quiet" {
            cfg.quiet = true;
        } else if arg == "-l" || arg == "--line-mode" {
            cfg.line_mode = true;
        } else if arg == "-W" || arg == "--wait" {
            cfg.wait_first_byte = true;
        } else if arg == "-f" || arg == "--force" {
            cfg.force = true;
        } else if arg == "-c" || arg == "--cursor" {
            cfg.cursor = true;
        } else if arg == "-s" || arg == "--size" {
            i += 1;
            let val = args.get(i).ok_or("-s requires a SIZE argument")?;
            cfg.expected_size = Some(parse_size(val)?);
        } else if let Some(rest) = arg.strip_prefix("--size=") {
            cfg.expected_size = Some(parse_size(rest)?);
        } else if arg == "-L" || arg == "--rate-limit" {
            i += 1;
            let val = args.get(i).ok_or("-L requires a RATE argument")?;
            cfg.rate_limit = Some(parse_size(val)?);
        } else if let Some(rest) = arg.strip_prefix("--rate-limit=") {
            cfg.rate_limit = Some(parse_size(rest)?);
        } else if arg == "-B" || arg == "--buffer-size" {
            i += 1;
            let val = args.get(i).ok_or("-B requires a SIZE argument")?;
            cfg.buffer_size = parse_size(val)? as usize;
        } else if let Some(rest) = arg.strip_prefix("--buffer-size=") {
            cfg.buffer_size = parse_size(rest)? as usize;
        } else if arg == "-N" {
            i += 1;
            let val = args.get(i).ok_or("-N requires a NAME argument")?.clone();
            cfg.name = Some(val);
        } else if arg.starts_with('-') && arg != "-" {
            return Err(format!("unknown option: {arg}"));
        } else {
            cfg.files.push(arg.clone());
        }
        i += 1;
    }

    Ok(cfg)
}

fn print_pv_usage() {
    eprintln!("Usage: pv [OPTIONS] [FILE...]");
    eprintln!("Monitor the progress of data through a pipe.");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  -s SIZE, --size=SIZE    Expected total size (for % and ETA)");
    eprintln!("  -n, --numeric           Output percentage on stderr for scripts");
    eprintln!("  -q, --quiet             No progress output");
    eprintln!("  -l, --line-mode         Count lines instead of bytes");
    eprintln!("  -L RATE, --rate-limit=RATE  Limit throughput (bytes/sec, K/M/G suffix)");
    eprintln!("  -B SIZE, --buffer-size=SIZE  Read buffer size (default 128K)");
    eprintln!("  -N NAME                 Name for progress display");
    eprintln!("  -W, --wait              Wait for first byte before showing progress");
    eprintln!("  -f, --force             Force output even if stderr is not a tty");
    eprintln!("  -c, --cursor            Use cursor positioning instead of CR");
    eprintln!("  -h, --help              Show this help");
}

fn run_pv(args: &[String]) -> Result<(), String> {
    let cfg = parse_pv_args(args)?;

    let mut stdout = io::stdout().lock();
    let mut stderr = io::stderr();

    let mut total_bytes: u64 = 0;
    let mut total_lines: u64 = 0;
    let start = Instant::now();
    let mut last_update = Instant::now();
    let mut first_byte_received = false;

    // Determine the list of readable sources.
    let sources: Vec<Box<dyn Read>> = if cfg.files.is_empty() || cfg.files == ["-"] {
        vec![Box::new(io::stdin().lock())]
    } else {
        let mut v: Vec<Box<dyn Read>> = Vec::new();
        for f in &cfg.files {
            if f == "-" {
                v.push(Box::new(io::stdin().lock()));
            } else {
                let file =
                    File::open(f).map_err(|e| format!("cannot open {}: {e}", quoteaf_os(f)))?;
                v.push(Box::new(file));
            }
        }
        v
    };

    let mut buf = vec![0u8; cfg.buffer_size];

    // Rate limiting state: track bytes sent in the current second-window.
    let mut rate_window_start = Instant::now();
    let mut rate_window_bytes: u64 = 0;

    for mut source in sources {
        loop {
            let n = source
                .read(&mut buf)
                .map_err(|e| format!("read error: {e}"))?;
            if n == 0 {
                break;
            }

            if !first_byte_received {
                first_byte_received = true;
            }

            stdout
                .write_all(&buf[..n])
                .map_err(|e| format!("write error: {e}"))?;

            total_bytes += n as u64;

            if cfg.line_mode {
                total_lines += buf[..n].iter().filter(|&&b| b == b'\n').count() as u64;
            }

            // Rate limiting
            if let Some(limit) = cfg.rate_limit {
                rate_window_bytes += n as u64;
                let window_elapsed = rate_window_start.elapsed().as_millis();
                if window_elapsed < 1000 {
                    if rate_window_bytes >= limit {
                        let sleep_ms = 1000u128.saturating_sub(window_elapsed);
                        if sleep_ms > 0 {
                            std::thread::sleep(std::time::Duration::from_millis(sleep_ms as u64));
                        }
                        rate_window_start = Instant::now();
                        rate_window_bytes = 0;
                    }
                } else {
                    rate_window_start = Instant::now();
                    rate_window_bytes = 0;
                }
            }

            // Progress display
            if !cfg.quiet
                && (!cfg.wait_first_byte || first_byte_received)
                && last_update.elapsed().as_millis() >= PROGRESS_INTERVAL_MS
            {
                last_update = Instant::now();
                let elapsed_ms = start.elapsed().as_millis();

                if cfg.numeric {
                    if let Some(total) = cfg.expected_size {
                        let pct = if total == 0 {
                            100
                        } else {
                            ((total_bytes as f64 / total as f64) * 100.0).min(100.0) as u64
                        };
                        let _ = writeln!(stderr, "{pct}");
                    }
                } else {
                    let count = if cfg.line_mode {
                        total_lines
                    } else {
                        total_bytes
                    };
                    let bar = render_progress_bar(
                        count,
                        cfg.expected_size,
                        elapsed_ms,
                        DEFAULT_BAR_WIDTH,
                        cfg.name.as_deref(),
                    );

                    if cfg.cursor {
                        let _ = write!(stderr, "\x1b[s\x1b[999;1H{bar}\x1b[u");
                    } else {
                        let _ = write!(stderr, "\r{bar}");
                    }
                }
            }
        }
    }

    // Final progress line
    if !cfg.quiet {
        let elapsed_ms = start.elapsed().as_millis();
        if cfg.numeric {
            if cfg.expected_size.is_some() {
                let _ = writeln!(stderr, "100");
            }
        } else {
            let count = if cfg.line_mode {
                total_lines
            } else {
                total_bytes
            };
            let bar = render_progress_bar(
                count,
                cfg.expected_size,
                elapsed_ms,
                DEFAULT_BAR_WIDTH,
                cfg.name.as_deref(),
            );
            if cfg.cursor {
                let _ = writeln!(stderr, "\x1b[s\x1b[999;1H{bar}\x1b[u");
            } else {
                let _ = writeln!(stderr, "\r{bar}");
            }
        }
    }

    stdout.flush().map_err(|e| format!("flush error: {e}"))?;
    Ok(())
}

// ============================================================================
// Main entry point
// ============================================================================

fn main() {
    let tool_args: Vec<String> = env::args().skip(1).collect();
    if let Err(e) = run_pv(&tool_args) {
        eprintln!("pv: {e}");
        process::exit(1);
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_size_bare_number() {
        assert_eq!(parse_size("1024").unwrap(), 1024);
    }

    #[test]
    fn test_parse_size_kilobytes() {
        assert_eq!(parse_size("4K").unwrap(), 4096);
    }

    #[test]
    fn test_parse_size_megabytes() {
        assert_eq!(parse_size("10M").unwrap(), 10 * 1024 * 1024);
    }

    #[test]
    fn test_parse_size_gigabytes() {
        assert_eq!(parse_size("2G").unwrap(), 2 * 1024 * 1024 * 1024);
    }

    #[test]
    fn test_parse_size_terabytes() {
        assert_eq!(parse_size("1T").unwrap(), 1024u64 * 1024 * 1024 * 1024);
    }

    #[test]
    fn test_parse_size_lowercase_k() {
        assert_eq!(parse_size("8k").unwrap(), 8192);
    }

    #[test]
    fn test_parse_size_kb_suffix() {
        assert_eq!(parse_size("5KB").unwrap(), 5120);
    }

    #[test]
    fn test_parse_size_bytes_suffix() {
        assert_eq!(parse_size("100B").unwrap(), 100);
    }

    #[test]
    fn test_parse_size_zero() {
        assert_eq!(parse_size("0").unwrap(), 0);
    }

    #[test]
    fn test_parse_size_empty_error() {
        assert!(parse_size("").is_err());
    }

    #[test]
    fn test_parse_size_invalid_number() {
        assert!(parse_size("abc").is_err());
    }

    #[test]
    fn test_parse_size_unknown_suffix() {
        assert!(parse_size("10X").is_err());
    }

    // -- Human-readable size formatting ---------------------------------------

    #[test]
    fn test_format_size_zero() {
        assert_eq!(format_size(0), "0B");
    }

    #[test]
    fn test_format_size_bytes() {
        assert_eq!(format_size(512), "512B");
    }

    #[test]
    fn test_format_size_kilobytes() {
        assert_eq!(format_size(1024), "1.00KB");
    }

    #[test]
    fn test_format_size_megabytes() {
        assert_eq!(format_size(10 * 1024 * 1024), "10.0MB");
    }

    #[test]
    fn test_format_size_gigabytes() {
        assert_eq!(format_size(2 * 1024 * 1024 * 1024), "2.00GB");
    }

    #[test]
    fn test_format_size_large_kb() {
        // 500 * 1024 = 512000 => 500KB
        assert_eq!(format_size(500 * 1024), "500KB");
    }

    // -- Time formatting ------------------------------------------------------

    #[test]
    fn test_format_time_zero() {
        assert_eq!(format_time(0), "0:00:00");
    }

    #[test]
    fn test_format_time_seconds() {
        assert_eq!(format_time(45), "0:00:45");
    }

    #[test]
    fn test_format_time_minutes() {
        assert_eq!(format_time(125), "0:02:05");
    }

    #[test]
    fn test_format_time_hours() {
        assert_eq!(format_time(3661), "1:01:01");
    }

    #[test]
    fn test_format_time_large() {
        assert_eq!(format_time(86399), "23:59:59");
    }

    // -- Throughput calculation ------------------------------------------------

    #[test]
    fn test_throughput_zero_time() {
        assert_eq!(calc_throughput(1000, 0), 0.0);
    }

    #[test]
    fn test_throughput_one_second() {
        let tp = calc_throughput(1_000_000, 1000);
        assert!((tp - 1_000_000.0).abs() < 0.1);
    }

    #[test]
    fn test_throughput_half_second() {
        let tp = calc_throughput(500, 500);
        assert!((tp - 1000.0).abs() < 0.1);
    }

    // -- ETA calculation ------------------------------------------------------

    #[test]
    fn test_eta_zero_remaining() {
        assert_eq!(calc_eta(0, 1000.0), Some(0));
    }

    #[test]
    fn test_eta_zero_throughput() {
        assert_eq!(calc_eta(1000, 0.0), Some(0));
    }

    #[test]
    fn test_eta_normal() {
        // 1MB remaining at 500KB/s => 2 seconds
        let eta = calc_eta(1_000_000, 500_000.0);
        assert_eq!(eta, Some(2));
    }

    #[test]
    fn test_eta_overflow_returns_none() {
        // Very large remaining, very slow speed
        assert!(calc_eta(u64::MAX, 0.001).is_none());
    }

    // -- Progress bar rendering -----------------------------------------------

    #[test]
    fn test_progress_bar_with_total() {
        let bar = render_progress_bar(50, Some(100), 1000, 10, None);
        assert!(bar.contains("50%"));
        assert!(bar.contains('['));
        assert!(bar.contains(']'));
    }

    #[test]
    fn test_progress_bar_full() {
        let bar = render_progress_bar(100, Some(100), 1000, 10, None);
        assert!(bar.contains("100%"));
        assert!(bar.contains("[==========]"));
    }

    #[test]
    fn test_progress_bar_empty() {
        let bar = render_progress_bar(0, Some(100), 1000, 10, None);
        assert!(bar.contains("0%"));
    }

    #[test]
    fn test_progress_bar_no_total() {
        let bar = render_progress_bar(1024, None, 2000, 10, None);
        // Should show size and throughput but no percentage
        assert!(bar.contains("1.00KB"));
        assert!(!bar.contains('%'));
    }

    #[test]
    fn test_progress_bar_with_name() {
        let bar = render_progress_bar(500, Some(1000), 1000, 10, Some("myfile"));
        assert!(bar.starts_with("myfile: "));
    }

    #[test]
    fn test_progress_bar_zero_total() {
        let bar = render_progress_bar(0, Some(0), 1000, 10, None);
        assert!(bar.contains("100%"));
    }

    // -- Proc fd path parsing -------------------------------------------------

    #[test]
    fn test_line_count_in_buffer() {
        let buf = b"line1\nline2\nline3\n";
        let count = buf.iter().filter(|&&b| b == b'\n').count();
        assert_eq!(count, 3);
    }

    #[test]
    fn test_line_count_no_newlines() {
        let buf = b"no newline here";
        let count = buf.iter().filter(|&&b| b == b'\n').count();
        assert_eq!(count, 0);
    }

    // -- Buffer size parsing --------------------------------------------------

    #[test]
    fn test_buffer_size_default() {
        assert_eq!(DEFAULT_BUFFER_SIZE, 128 * 1024);
    }

    #[test]
    fn test_buffer_size_parse_256k() {
        assert_eq!(parse_size("256K").unwrap(), 256 * 1024);
    }

    #[test]
    fn test_buffer_size_parse_1m() {
        assert_eq!(parse_size("1M").unwrap(), 1024 * 1024);
    }

    // -- Edge cases -----------------------------------------------------------

    #[test]
    fn test_parse_size_overflow() {
        // Attempting a ridiculously large size with suffix
        assert!(parse_size("999999999999999999E").is_err());
    }

    #[test]
    fn test_format_size_one() {
        assert_eq!(format_size(1), "1B");
    }

    #[test]
    fn test_format_size_just_under_kb() {
        assert_eq!(format_size(1023), "1023B");
    }

    #[test]
    fn test_split_number_suffix_no_suffix() {
        assert_eq!(split_number_suffix("12345"), ("12345", ""));
    }

    #[test]
    fn test_split_number_suffix_with_suffix() {
        assert_eq!(split_number_suffix("100M"), ("100", "M"));
    }

    #[test]
    fn test_split_number_suffix_multi_char() {
        assert_eq!(split_number_suffix("50KB"), ("50", "KB"));
    }

    // -- PV argument parsing --------------------------------------------------

    #[test]
    fn test_pv_parse_quiet() {
        let args = vec!["-q".to_string()];
        let cfg = parse_pv_args(&args).unwrap();
        assert!(cfg.quiet);
    }

    #[test]
    fn test_pv_parse_size() {
        let args = vec!["-s".to_string(), "10M".to_string()];
        let cfg = parse_pv_args(&args).unwrap();
        assert_eq!(cfg.expected_size, Some(10 * 1024 * 1024));
    }

    #[test]
    fn test_pv_parse_line_mode() {
        let args = vec!["-l".to_string()];
        let cfg = parse_pv_args(&args).unwrap();
        assert!(cfg.line_mode);
    }

    #[test]
    fn test_pv_parse_name() {
        let args = vec!["-N".to_string(), "backup".to_string()];
        let cfg = parse_pv_args(&args).unwrap();
        assert_eq!(cfg.name, Some("backup".to_string()));
    }

    #[test]
    fn test_pv_parse_rate_limit() {
        let args = vec!["-L".to_string(), "1M".to_string()];
        let cfg = parse_pv_args(&args).unwrap();
        assert_eq!(cfg.rate_limit, Some(1024 * 1024));
    }

    #[test]
    fn test_pv_parse_buffer_size() {
        let args = vec!["-B".to_string(), "256K".to_string()];
        let cfg = parse_pv_args(&args).unwrap();
        assert_eq!(cfg.buffer_size, 256 * 1024);
    }

    #[test]
    fn test_pv_parse_files() {
        let args = vec!["a.txt".to_string(), "b.txt".to_string()];
        let cfg = parse_pv_args(&args).unwrap();
        assert_eq!(cfg.files, vec!["a.txt", "b.txt"]);
    }

    #[test]
    fn test_pv_parse_unknown_flag() {
        let args = vec!["--bogus".to_string()];
        assert!(parse_pv_args(&args).is_err());
    }
}
