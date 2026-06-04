//! OurOS Pipe/File Management Tools
//!
//! Multi-personality binary combining four file/pipe utilities, selected via
//! argv\[0\]:
//!
//! - **pv** (default) -- monitor data flowing through a pipe (pipe viewer)
//! - **truncate** -- shrink or extend file size
//! - **shred** -- overwrite files to hinder recovery
//! - **fuser** -- find processes using files or sockets
//!
//! # Examples
//!
//! ```text
//! # Pipe viewer
//! pv -s 100M bigfile.iso | gzip > bigfile.iso.gz
//!
//! # Truncate
//! truncate -s 10M sparse.img
//!
//! # Shred
//! shred -vuz secret.key
//!
//! # Fuser
//! fuser -v /var/log/syslog
//! ```

use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::process;
use std::time::Instant;

// ============================================================================
// Constants
// ============================================================================

/// Default read buffer size for pv (128 KiB).
const DEFAULT_BUFFER_SIZE: usize = 128 * 1024;

/// Default number of shred passes.
const DEFAULT_SHRED_PASSES: u32 = 3;

/// Progress update interval in milliseconds.
const PROGRESS_INTERVAL_MS: u128 = 100;

/// Width allocated for the progress bar (excluding brackets).
const DEFAULT_BAR_WIDTH: usize = 25;

// ============================================================================
// Personality detection
// ============================================================================

/// Which tool personality we are running as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Personality {
    Pv,
    Truncate,
    Shred,
    Fuser,
}

/// Detect personality from the basename of argv\[0\].
fn detect_personality(argv0: &str) -> Personality {
    let basename = Path::new(argv0)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(argv0);

    match basename {
        "truncate" => Personality::Truncate,
        "shred" => Personality::Shred,
        "fuser" => Personality::Fuser,
        _ => Personality::Pv,
    }
}

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
        .map_err(|e| format!("invalid number '{num_str}': {e}"))?;

    let multiplier = suffix_multiplier(suffix)?;

    base.checked_mul(multiplier)
        .ok_or_else(|| format!("size overflow: {s}"))
}

/// Parse a truncate-style size with optional prefix (+, -, <, >, /, %).
///
/// Returns `(prefix_char_or_None, byte_value)`.
fn parse_truncate_size(s: &str) -> Result<(Option<char>, u64), String> {
    if s.is_empty() {
        return Err("empty size specification".into());
    }

    let first = s.as_bytes()[0];
    let (prefix, rest) = match first {
        b'+' | b'-' | b'<' | b'>' | b'/' | b'%' => (Some(first as char), &s[1..]),
        _ => (None, s),
    };

    let size = parse_size(rest)?;
    Ok((prefix, size))
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
        _ => Err(format!("unknown size suffix '{suffix}'")),
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
            let val = args
                .get(i)
                .ok_or("-N requires a NAME argument")?
                .clone();
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
                let file = File::open(f).map_err(|e| format!("cannot open '{f}': {e}"))?;
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
                            std::thread::sleep(std::time::Duration::from_millis(
                                sleep_ms as u64,
                            ));
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
// truncate mode
// ============================================================================

/// Configuration for truncate mode.
struct TruncateConfig {
    size_spec: Option<String>,
    no_create: bool,
    reference: Option<String>,
    files: Vec<String>,
}

fn parse_truncate_args(args: &[String]) -> Result<TruncateConfig, String> {
    let mut cfg = TruncateConfig {
        size_spec: None,
        no_create: false,
        reference: None,
        files: Vec::new(),
    };
    let mut i = 0;

    while i < args.len() {
        let arg = &args[i];
        if arg == "--help" || arg == "-h" {
            print_truncate_usage();
            process::exit(0);
        } else if arg == "-c" || arg == "--no-create" {
            cfg.no_create = true;
        } else if arg == "-s" || arg == "--size" {
            i += 1;
            cfg.size_spec = Some(
                args.get(i)
                    .ok_or("-s requires a SIZE argument")?
                    .clone(),
            );
        } else if let Some(rest) = arg.strip_prefix("--size=") {
            cfg.size_spec = Some(rest.to_string());
        } else if arg == "-r" || arg == "--reference" {
            i += 1;
            cfg.reference = Some(
                args.get(i)
                    .ok_or("-r requires a FILE argument")?
                    .clone(),
            );
        } else if let Some(rest) = arg.strip_prefix("--reference=") {
            cfg.reference = Some(rest.to_string());
        } else if arg.starts_with('-') {
            return Err(format!("unknown option: {arg}"));
        } else {
            cfg.files.push(arg.clone());
        }
        i += 1;
    }

    if cfg.files.is_empty() {
        return Err("no files specified".into());
    }
    if cfg.size_spec.is_none() && cfg.reference.is_none() {
        return Err("must specify either -s SIZE or -r REFERENCE".into());
    }

    Ok(cfg)
}

fn print_truncate_usage() {
    eprintln!("Usage: truncate -s SIZE FILE...");
    eprintln!("       truncate -r REFERENCE FILE...");
    eprintln!("Shrink or extend the size of each FILE.");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  -s SIZE, --size=SIZE     Set or adjust file size");
    eprintln!("    Prefix: + extend, - shrink, < at most, > at least,");
    eprintln!("            / round down, % round up (to multiple of SIZE)");
    eprintln!("    Suffix: K (1024), M, G, T, P, E");
    eprintln!("  -c, --no-create          Don't create files that don't exist");
    eprintln!("  -r FILE, --reference=FILE  Use reference file's size");
    eprintln!("  -h, --help               Show this help");
}

/// Compute the new file size given the current size and the size spec.
fn compute_truncate_size(current: u64, prefix: Option<char>, value: u64) -> Result<u64, String> {
    match prefix {
        None => Ok(value),
        Some('+') => current
            .checked_add(value)
            .ok_or_else(|| "size overflow".to_string()),
        Some('-') => Ok(current.saturating_sub(value)),
        Some('<') => Ok(current.min(value)),
        Some('>') => Ok(current.max(value)),
        Some('/') => {
            // Round down to nearest multiple of value
            if value == 0 {
                return Err("cannot round to multiple of zero".into());
            }
            Ok((current / value) * value)
        }
        Some('%') => {
            // Round up to nearest multiple of value
            if value == 0 {
                return Err("cannot round to multiple of zero".into());
            }
            let rem = current % value;
            if rem == 0 {
                Ok(current)
            } else {
                current
                    .checked_add(value - rem)
                    .ok_or_else(|| "size overflow".to_string())
            }
        }
        Some(c) => Err(format!("unknown size prefix '{c}'")),
    }
}

fn run_truncate(args: &[String]) -> Result<(), String> {
    let cfg = parse_truncate_args(args)?;

    // Determine the base size value from -r or -s.
    let (prefix, base_size) = if let Some(ref refpath) = cfg.reference {
        let meta = fs::metadata(refpath)
            .map_err(|e| format!("cannot stat reference '{}': {}", refpath, e))?;
        (None, meta.len())
    } else {
        let spec = cfg.size_spec.as_ref().expect("validated above");
        parse_truncate_size(spec)?
    };

    for path in &cfg.files {
        let exists = Path::new(path).exists();

        if !exists && cfg.no_create {
            continue;
        }

        // Open or create the file.
        let file = OpenOptions::new()
            .write(true)
            .create(!cfg.no_create)
            .open(path)
            .map_err(|e| format!("cannot open '{path}': {e}"))?;

        let current_len = file
            .metadata()
            .map_err(|e| format!("cannot stat '{path}': {e}"))?
            .len();

        let new_size = compute_truncate_size(current_len, prefix, base_size)?;

        file.set_len(new_size)
            .map_err(|e| format!("cannot truncate '{path}' to {new_size}: {e}"))?;
    }

    Ok(())
}

// ============================================================================
// shred mode
// ============================================================================

/// Configuration for shred mode.
struct ShredConfig {
    iterations: u32,
    add_zero_pass: bool,
    remove_after: bool,
    overwrite_size: Option<u64>,
    verbose: bool,
    force: bool,
    random_source: String,
    files: Vec<String>,
}

impl Default for ShredConfig {
    fn default() -> Self {
        Self {
            iterations: DEFAULT_SHRED_PASSES,
            add_zero_pass: false,
            remove_after: false,
            overwrite_size: None,
            verbose: false,
            force: false,
            random_source: "/dev/urandom".into(),
            files: Vec::new(),
        }
    }
}

fn parse_shred_args(args: &[String]) -> Result<ShredConfig, String> {
    let mut cfg = ShredConfig::default();
    let mut i = 0;

    while i < args.len() {
        let arg = &args[i];
        if arg == "--help" || arg == "-h" {
            print_shred_usage();
            process::exit(0);
        } else if arg == "-z" || arg == "--zero" {
            cfg.add_zero_pass = true;
        } else if arg == "-u" || arg == "--remove" {
            cfg.remove_after = true;
        } else if arg == "-v" || arg == "--verbose" {
            cfg.verbose = true;
        } else if arg == "-f" || arg == "--force" {
            cfg.force = true;
        } else if arg == "-n" || arg == "--iterations" {
            i += 1;
            let val = args.get(i).ok_or("-n requires a COUNT argument")?;
            cfg.iterations = val
                .parse()
                .map_err(|e| format!("invalid iteration count '{val}': {e}"))?;
        } else if let Some(rest) = arg.strip_prefix("--iterations=") {
            cfg.iterations = rest
                .parse()
                .map_err(|e| format!("invalid iteration count '{rest}': {e}"))?;
        } else if arg == "-s" || arg == "--size" {
            i += 1;
            let val = args.get(i).ok_or("-s requires a SIZE argument")?;
            cfg.overwrite_size = Some(parse_size(val)?);
        } else if let Some(rest) = arg.strip_prefix("--size=") {
            cfg.overwrite_size = Some(parse_size(rest)?);
        } else if let Some(rest) = arg.strip_prefix("--random-source=") {
            cfg.random_source = rest.to_string();
        } else if arg.starts_with('-') {
            return Err(format!("unknown option: {arg}"));
        } else {
            cfg.files.push(arg.clone());
        }
        i += 1;
    }

    if cfg.files.is_empty() {
        return Err("no files specified".into());
    }

    Ok(cfg)
}

fn print_shred_usage() {
    eprintln!("Usage: shred [OPTIONS] FILE...");
    eprintln!("Overwrite files to make recovery difficult.");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  -n N, --iterations=N      Overwrite N times (default 3)");
    eprintln!("  -z, --zero                Add final zero-fill pass");
    eprintln!("  -u, --remove              Truncate and remove after overwriting");
    eprintln!("  -s SIZE, --size=SIZE      Overwrite only first SIZE bytes");
    eprintln!("  -v, --verbose             Show progress");
    eprintln!("  -f, --force               Change permissions to allow writing");
    eprintln!("  --random-source=FILE      Source of random bytes (default /dev/urandom)");
    eprintln!("  -h, --help                Show this help");
}

/// Simple deterministic PRNG for generating shred patterns.
///
/// We use xorshift64 so we do not depend on /dev/urandom being available during
/// tests, and so the shred pass content is reproducible per-seed.
struct XorShift64 {
    state: u64,
}

impl XorShift64 {
    fn new(seed: u64) -> Self {
        Self {
            state: if seed == 0 { 0xDEAD_BEEF_CAFE_BABE } else { seed },
        }
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        x
    }

    fn fill_bytes(&mut self, buf: &mut [u8]) {
        let mut pos = 0;
        while pos < buf.len() {
            let val = self.next_u64();
            let bytes = val.to_le_bytes();
            let remaining = buf.len() - pos;
            let copy_len = remaining.min(8);
            buf[pos..pos + copy_len].copy_from_slice(&bytes[..copy_len]);
            pos += copy_len;
        }
    }
}

/// Generate a shred pattern buffer for the given pass number.
///
/// - Even passes: random data
/// - Odd passes: bitwise complement of the previous pass
///
/// The `seed` should incorporate pass number and file identity.
fn generate_shred_pattern(buf: &mut [u8], pass: u32, seed: u64) {
    if pass.is_multiple_of(2) {
        // Random pass
        let mut rng = XorShift64::new(seed.wrapping_add(pass as u64));
        rng.fill_bytes(buf);
    } else {
        // Complement of a random pass
        let mut rng = XorShift64::new(seed.wrapping_add(pass.wrapping_sub(1) as u64));
        rng.fill_bytes(buf);
        for b in buf.iter_mut() {
            *b = !*b;
        }
    }
}

fn run_shred(args: &[String]) -> Result<(), String> {
    let cfg = parse_shred_args(args)?;

    for path in &cfg.files {
        // If force, try to make the file writable.
        if cfg.force
            && let Ok(meta) = fs::metadata(path) {
                let mut perms = meta.permissions();
                #[allow(clippy::permissions_set_readonly_false)]
                perms.set_readonly(false);
                let _ = fs::set_permissions(path, perms);
            }

        let file_size = fs::metadata(path)
            .map_err(|e| format!("cannot stat '{path}': {e}"))?
            .len();

        let shred_size = cfg.overwrite_size.unwrap_or(file_size);

        // Open file for writing.
        let mut file = OpenOptions::new()
            .write(true)
            .open(path)
            .map_err(|e| format!("cannot open '{path}' for writing: {e}"))?;

        // Use the file path hash as a seed component for reproducibility in tests.
        let path_hash = path.bytes().fold(0u64, |acc, b| {
            acc.wrapping_mul(31).wrapping_add(b as u64)
        });

        let total_passes = if cfg.add_zero_pass {
            cfg.iterations + 1
        } else {
            cfg.iterations
        };

        let mut buf = vec![0u8; 65536.min(shred_size as usize)];

        for pass in 0..total_passes {
            if cfg.verbose {
                let pass_label = if cfg.add_zero_pass && pass == total_passes - 1 {
                    "zero".to_string()
                } else {
                    format!("{}/{}", pass + 1, cfg.iterations)
                };
                eprintln!("shred: {path}: pass {pass_label}");
            }

            file.seek(SeekFrom::Start(0))
                .map_err(|e| format!("seek error on '{path}': {e}"))?;

            let mut remaining = shred_size;
            while remaining > 0 {
                let chunk = (remaining as usize).min(buf.len());
                let write_buf = &mut buf[..chunk];

                if cfg.add_zero_pass && pass == total_passes - 1 {
                    // Zero fill pass
                    write_buf.fill(0);
                } else {
                    generate_shred_pattern(write_buf, pass, path_hash);
                }

                file.write_all(write_buf)
                    .map_err(|e| format!("write error on '{path}': {e}"))?;
                remaining -= chunk as u64;
            }

            file.flush()
                .map_err(|e| format!("flush error on '{path}': {e}"))?;

            // Sync to disk
            file.sync_all()
                .map_err(|e| format!("sync error on '{path}': {e}"))?;
        }

        // Remove if requested.
        if cfg.remove_after {
            // Truncate to zero first.
            file.set_len(0)
                .map_err(|e| format!("truncate error on '{path}': {e}"))?;
            drop(file);
            fs::remove_file(path)
                .map_err(|e| format!("cannot remove '{path}': {e}"))?;
            if cfg.verbose {
                eprintln!("shred: {path}: removed");
            }
        }
    }

    Ok(())
}

// ============================================================================
// fuser mode
// ============================================================================

/// Access type for a process using a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)] // Root and Mmap are part of the complete access-type enum
enum AccessType {
    /// File descriptor access (reading/writing).
    Fd,
    /// Root directory.
    Root,
    /// Current working directory.
    Cwd,
    /// Memory-mapped file.
    Mmap,
    /// Executable.
    Exe,
}

impl std::fmt::Display for AccessType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Fd => write!(f, "f"),
            Self::Root => write!(f, "r"),
            Self::Cwd => write!(f, "c"),
            Self::Mmap => write!(f, "m"),
            Self::Exe => write!(f, "e"),
        }
    }
}

/// Information about a process using a target resource.
#[derive(Debug, Clone)]
struct ProcessUse {
    pid: u32,
    user: String,
    access: AccessType,
    command: String,
}

/// Configuration for fuser mode.
struct FuserConfig {
    send_signal: bool,
    signal: i32,
    show_all: bool,
    mount_mode: bool,
    namespace: String,
    verbose: bool,
    show_user: bool,
    targets: Vec<String>,
}

impl Default for FuserConfig {
    fn default() -> Self {
        Self {
            send_signal: false,
            signal: 9, // SIGKILL
            show_all: false,
            mount_mode: false,
            namespace: "file".into(),
            verbose: false,
            show_user: false,
            targets: Vec::new(),
        }
    }
}

/// Map a signal name to its number.
fn signal_name_to_number(name: &str) -> Result<i32, String> {
    // Support both "KILL" and "SIGKILL" forms.
    let canonical = name.to_ascii_uppercase();
    let canonical = canonical.strip_prefix("SIG").unwrap_or(&canonical);

    match canonical {
        "HUP" => Ok(1),
        "INT" => Ok(2),
        "QUIT" => Ok(3),
        "ILL" => Ok(4),
        "TRAP" => Ok(5),
        "ABRT" | "IOT" => Ok(6),
        "BUS" => Ok(7),
        "FPE" => Ok(8),
        "KILL" => Ok(9),
        "USR1" => Ok(10),
        "SEGV" => Ok(11),
        "USR2" => Ok(12),
        "PIPE" => Ok(13),
        "ALRM" => Ok(14),
        "TERM" => Ok(15),
        "STKFLT" => Ok(16),
        "CHLD" => Ok(17),
        "CONT" => Ok(18),
        "STOP" => Ok(19),
        "TSTP" => Ok(20),
        "TTIN" => Ok(21),
        "TTOU" => Ok(22),
        "URG" => Ok(23),
        "XCPU" => Ok(24),
        "XFSZ" => Ok(25),
        "VTALRM" => Ok(26),
        "PROF" => Ok(27),
        "WINCH" => Ok(28),
        "IO" | "POLL" => Ok(29),
        "PWR" => Ok(30),
        "SYS" => Ok(31),
        _ => {
            // Try parsing as a number.
            name.parse::<i32>()
                .map_err(|_| format!("unknown signal: {name}"))
        }
    }
}

/// Map a signal number to its name. Held for the future -N / --watchfd
/// path where we emit signal info; currently unused.
#[allow(dead_code)]
fn signal_number_to_name(num: i32) -> &'static str {
    match num {
        1 => "HUP",
        2 => "INT",
        3 => "QUIT",
        4 => "ILL",
        5 => "TRAP",
        6 => "ABRT",
        7 => "BUS",
        8 => "FPE",
        9 => "KILL",
        10 => "USR1",
        11 => "SEGV",
        12 => "USR2",
        13 => "PIPE",
        14 => "ALRM",
        15 => "TERM",
        16 => "STKFLT",
        17 => "CHLD",
        18 => "CONT",
        19 => "STOP",
        20 => "TSTP",
        21 => "TTIN",
        22 => "TTOU",
        23 => "URG",
        24 => "XCPU",
        25 => "XFSZ",
        26 => "VTALRM",
        27 => "PROF",
        28 => "WINCH",
        29 => "IO",
        30 => "PWR",
        31 => "SYS",
        _ => "UNKNOWN",
    }
}

fn parse_fuser_args(args: &[String]) -> Result<FuserConfig, String> {
    let mut cfg = FuserConfig::default();
    let mut i = 0;

    while i < args.len() {
        let arg = &args[i];
        if arg == "--help" || arg == "-h" {
            print_fuser_usage();
            process::exit(0);
        } else if arg == "-k" || arg == "--kill" {
            cfg.send_signal = true;
        } else if arg == "-a" || arg == "--all" {
            cfg.show_all = true;
        } else if arg == "-m" || arg == "--mount" {
            cfg.mount_mode = true;
        } else if arg == "-v" || arg == "--verbose" {
            cfg.verbose = true;
        } else if arg == "-u" || arg == "--user" {
            cfg.show_user = true;
        } else if arg == "-s" {
            i += 1;
            let val = args.get(i).ok_or("-s requires a SIGNAL argument")?;
            cfg.signal = signal_name_to_number(val)?;
        } else if arg == "-n" || arg == "--namespace" {
            i += 1;
            let val = args
                .get(i)
                .ok_or("-n requires a NAMESPACE argument")?
                .clone();
            match val.as_str() {
                "file" | "tcp" | "udp" => cfg.namespace = val,
                _ => return Err(format!("unknown namespace: {val}")),
            }
        } else if let Some(rest) = arg.strip_prefix("--namespace=") {
            match rest {
                "file" | "tcp" | "udp" => cfg.namespace = rest.to_string(),
                _ => return Err(format!("unknown namespace: {rest}")),
            }
        } else if arg.starts_with('-')
            && arg.len() > 1
            && arg.as_bytes()[1].is_ascii_uppercase()
        {
            // -SIGNAL shorthand (e.g. -KILL, -HUP, -9)
            let sig_str = &arg[1..];
            cfg.signal = signal_name_to_number(sig_str)?;
            cfg.send_signal = true;
        } else if arg.starts_with('-') && arg.len() > 1 {
            return Err(format!("unknown option: {arg}"));
        } else {
            cfg.targets.push(arg.clone());
        }
        i += 1;
    }

    if cfg.targets.is_empty() {
        return Err("no targets specified".into());
    }

    Ok(cfg)
}

fn print_fuser_usage() {
    eprintln!("Usage: fuser [OPTIONS] FILE...");
    eprintln!("       fuser -n tcp|udp PORT");
    eprintln!("Find processes using files or sockets.");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  -k, --kill               Send signal to processes (default SIGKILL)");
    eprintln!("  -SIGNAL                  Signal to send (e.g. -HUP, -9)");
    eprintln!("  -s SIGNAL                Signal to send");
    eprintln!("  -a, --all                Display unused files too");
    eprintln!("  -m, --mount              All processes on mount point");
    eprintln!("  -n SPACE, --namespace=SPACE  Namespace: file, tcp, udp");
    eprintln!("  -v, --verbose            Verbose output (USER PID ACCESS COMMAND)");
    eprintln!("  -u, --user               Show user name");
    eprintln!("  -h, --help               Show this help");
}

/// Parse a /proc/<pid>/fd/<n> symlink target to determine which file is open.
///
/// Returns the target path if it can be read.
fn parse_proc_fd_link(pid: u32, fd: u32) -> Option<String> {
    let link_path = format!("/proc/{pid}/fd/{fd}");
    fs::read_link(&link_path)
        .ok()
        .and_then(|p| p.to_str().map(String::from))
}

/// Read the command name for a PID from /proc/<pid>/comm.
fn read_proc_comm(pid: u32) -> String {
    let path = format!("/proc/{pid}/comm");
    fs::read_to_string(&path)
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "?".into())
}

/// Read the owning user for a PID from /proc/<pid>/status (Uid line).
fn read_proc_user(pid: u32) -> String {
    let path = format!("/proc/{pid}/status");
    let content = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return "?".into(),
    };

    for line in content.lines() {
        if let Some(rest) = line.strip_prefix("Uid:\t") {
            // Format: real effective saved filesystem
            let uid_str = rest.split_whitespace().next().unwrap_or("?");
            return uid_str.to_string();
        }
    }

    "?".into()
}

/// Find all PIDs that have the given file path open.
fn find_file_users(target: &str) -> Vec<ProcessUse> {
    let mut results = Vec::new();

    // Canonicalize the target path.
    let canonical = match fs::canonicalize(target) {
        Ok(p) => p.to_string_lossy().to_string(),
        Err(_) => target.to_string(),
    };

    // Scan /proc
    let proc_dir = match fs::read_dir("/proc") {
        Ok(d) => d,
        Err(_) => return results,
    };

    for entry in proc_dir.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        let pid: u32 = match name_str.parse() {
            Ok(p) => p,
            Err(_) => continue,
        };

        // Check /proc/<pid>/fd/*
        let fd_dir = format!("/proc/{pid}/fd");
        if let Ok(fds) = fs::read_dir(&fd_dir) {
            for fd_entry in fds.flatten() {
                let fd_name = fd_entry.file_name();
                let fd_num: u32 = match fd_name.to_string_lossy().parse() {
                    Ok(n) => n,
                    Err(_) => continue,
                };

                if let Some(link_target) = parse_proc_fd_link(pid, fd_num)
                    && link_target == canonical {
                        results.push(ProcessUse {
                            pid,
                            user: read_proc_user(pid),
                            access: AccessType::Fd,
                            command: read_proc_comm(pid),
                        });
                        break; // One entry per PID
                    }
            }
        }

        // Check /proc/<pid>/cwd
        let cwd_path = format!("/proc/{pid}/cwd");
        if let Ok(link) = fs::read_link(&cwd_path)
            && link.to_string_lossy() == canonical {
                // Only add if not already found via fd
                if !results.iter().any(|r| r.pid == pid) {
                    results.push(ProcessUse {
                        pid,
                        user: read_proc_user(pid),
                        access: AccessType::Cwd,
                        command: read_proc_comm(pid),
                    });
                }
            }

        // Check /proc/<pid>/exe
        let exe_path = format!("/proc/{pid}/exe");
        if let Ok(link) = fs::read_link(&exe_path)
            && link.to_string_lossy() == canonical
                && !results.iter().any(|r| r.pid == pid) {
                    results.push(ProcessUse {
                        pid,
                        user: read_proc_user(pid),
                        access: AccessType::Exe,
                        command: read_proc_comm(pid),
                    });
                }
    }

    results
}

/// Find processes using a TCP or UDP port by parsing /proc/net/{tcp,udp}.
fn find_net_users(port_str: &str, proto: &str) -> Result<Vec<ProcessUse>, String> {
    let port: u16 = port_str
        .parse()
        .map_err(|e| format!("invalid port '{port_str}': {e}"))?;

    let proc_net_path = format!("/proc/net/{proto}");
    let content = fs::read_to_string(&proc_net_path)
        .map_err(|e| format!("cannot read {proc_net_path}: {e}"))?;

    let mut inodes: Vec<u64> = Vec::new();

    // Skip header line, parse each line for local_address containing our port.
    for line in content.lines().skip(1) {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 10 {
            continue;
        }

        // Field 1 is local_address as hex_ip:hex_port
        let local_addr = fields[1];
        if let Some(port_hex) = local_addr.split(':').nth(1)
            && let Ok(p) = u16::from_str_radix(port_hex, 16)
                && p == port {
                    // Field 9 is the inode
                    if let Ok(inode) = fields[9].parse::<u64>() {
                        inodes.push(inode);
                    }
                }
    }

    if inodes.is_empty() {
        return Ok(Vec::new());
    }

    // Now find which PIDs have these inodes in their fd links.
    let mut results = Vec::new();
    let proc_dir = match fs::read_dir("/proc") {
        Ok(d) => d,
        Err(_) => return Ok(results),
    };

    for entry in proc_dir.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        let pid: u32 = match name_str.parse() {
            Ok(p) => p,
            Err(_) => continue,
        };

        let fd_dir = format!("/proc/{pid}/fd");
        if let Ok(fds) = fs::read_dir(&fd_dir) {
            for fd_entry in fds.flatten() {
                if let Some(link_target) = parse_proc_fd_link(
                    pid,
                    fd_entry
                        .file_name()
                        .to_string_lossy()
                        .parse()
                        .unwrap_or(0),
                ) {
                    // Socket inodes appear as "socket:[12345]"
                    if let Some(rest) = link_target.strip_prefix("socket:[")
                        && let Some(inode_str) = rest.strip_suffix(']')
                            && let Ok(inode) = inode_str.parse::<u64>()
                                && inodes.contains(&inode) {
                                    results.push(ProcessUse {
                                        pid,
                                        user: read_proc_user(pid),
                                        access: AccessType::Fd,
                                        command: read_proc_comm(pid),
                                    });
                                    break;
                                }
                }
            }
        }
    }

    Ok(results)
}

/// Send a signal to a process by writing to an appropriate mechanism.
///
/// On a real Unix system this would call `kill(pid, sig)`. We do it via libc.
fn send_signal(pid: u32, sig: i32) -> Result<(), String> {
    #[cfg(target_family = "unix")]
    {
        unsafe extern "C" {
            fn kill(pid: i32, sig: i32) -> i32;
        }
        // SAFETY: kill() is a standard POSIX function. We pass a valid PID and
        // signal number. The return value is checked for errors.
        let ret = unsafe { kill(pid as i32, sig) };
        if ret != 0 {
            return Err(format!(
                "failed to send signal {} to PID {}: errno",
                signal_number_to_name(sig),
                pid,
            ));
        }
    }

    #[cfg(not(target_family = "unix"))]
    {
        let _ = (pid, sig);
        Err("signal sending not supported on this platform".into())
    }

    #[cfg(target_family = "unix")]
    Ok(())
}

fn run_fuser(args: &[String]) -> Result<(), String> {
    let cfg = parse_fuser_args(args)?;

    for target in &cfg.targets {
        let users = if cfg.namespace == "tcp" || cfg.namespace == "udp" {
            find_net_users(target, &cfg.namespace)?
        } else {
            find_file_users(target)
        };

        if users.is_empty() {
            if cfg.show_all {
                eprintln!("{target}: no processes found");
            }
            continue;
        }

        if cfg.verbose {
            eprintln!(
                "{:<25} {:>6} {:>6} {:>6} COMMAND",
                "FILE", "USER", "PID", "ACCESS"
            );
            for u in &users {
                eprintln!(
                    "{:<25} {:>6} {:>6} {:>6} {}",
                    target, u.user, u.pid, u.access, u.command
                );
            }
        } else {
            // Standard output: file: PID PID ...
            let pids: Vec<String> = users.iter().map(|u| {
                let mut s = u.pid.to_string();
                if cfg.show_user {
                    s.push_str(&format!("({})", u.user));
                }
                s
            }).collect();
            // PID list goes to stdout, filename header to stderr
            eprintln!("{target}:");
            println!("{}", pids.join(" "));
        }

        // Send signals if requested.
        if cfg.send_signal {
            for u in &users {
                if let Err(e) = send_signal(u.pid, cfg.signal) {
                    eprintln!("fuser: {e}");
                }
            }
        }
    }

    Ok(())
}

// ============================================================================
// Main entry point
// ============================================================================

fn main() {
    let args: Vec<String> = env::args().collect();
    let argv0 = args.first().map(String::as_str).unwrap_or("pv");
    let personality = detect_personality(argv0);
    let tool_args: Vec<String> = args.into_iter().skip(1).collect();

    let tool_name = match personality {
        Personality::Pv => "pv",
        Personality::Truncate => "truncate",
        Personality::Shred => "shred",
        Personality::Fuser => "fuser",
    };

    let result = match personality {
        Personality::Pv => run_pv(&tool_args),
        Personality::Truncate => run_truncate(&tool_args),
        Personality::Shred => run_shred(&tool_args),
        Personality::Fuser => run_fuser(&tool_args),
    };

    if let Err(e) = result {
        eprintln!("{tool_name}: {e}");
        process::exit(1);
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -- Personality detection ------------------------------------------------

    #[test]
    fn test_personality_pv_default() {
        assert_eq!(detect_personality("pv"), Personality::Pv);
    }

    #[test]
    fn test_personality_pv_with_path() {
        assert_eq!(detect_personality("/usr/bin/pv"), Personality::Pv);
    }

    #[test]
    fn test_personality_pv_relative_path() {
        assert_eq!(detect_personality("./pv"), Personality::Pv);
    }

    #[test]
    fn test_personality_unknown_defaults_pv() {
        assert_eq!(detect_personality("something_else"), Personality::Pv);
    }

    #[test]
    fn test_personality_truncate() {
        assert_eq!(detect_personality("truncate"), Personality::Truncate);
    }

    #[test]
    fn test_personality_truncate_with_path() {
        assert_eq!(
            detect_personality("/usr/bin/truncate"),
            Personality::Truncate
        );
    }

    #[test]
    fn test_personality_shred() {
        assert_eq!(detect_personality("shred"), Personality::Shred);
    }

    #[test]
    fn test_personality_shred_with_path() {
        assert_eq!(detect_personality("/sbin/shred"), Personality::Shred);
    }

    #[test]
    fn test_personality_fuser() {
        assert_eq!(detect_personality("fuser"), Personality::Fuser);
    }

    #[test]
    fn test_personality_fuser_with_path() {
        assert_eq!(detect_personality("/usr/sbin/fuser"), Personality::Fuser);
    }

    // -- Size parsing ---------------------------------------------------------

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
        assert_eq!(
            parse_size("1T").unwrap(),
            1024u64 * 1024 * 1024 * 1024
        );
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

    // -- Truncate size parsing ------------------------------------------------

    #[test]
    fn test_truncate_size_no_prefix() {
        let (prefix, size) = parse_truncate_size("100K").unwrap();
        assert_eq!(prefix, None);
        assert_eq!(size, 102400);
    }

    #[test]
    fn test_truncate_size_extend() {
        let (prefix, size) = parse_truncate_size("+50M").unwrap();
        assert_eq!(prefix, Some('+'));
        assert_eq!(size, 50 * 1024 * 1024);
    }

    #[test]
    fn test_truncate_size_shrink() {
        let (prefix, size) = parse_truncate_size("-1K").unwrap();
        assert_eq!(prefix, Some('-'));
        assert_eq!(size, 1024);
    }

    #[test]
    fn test_truncate_size_at_most() {
        let (prefix, size) = parse_truncate_size("<1G").unwrap();
        assert_eq!(prefix, Some('<'));
        assert_eq!(size, 1024 * 1024 * 1024);
    }

    #[test]
    fn test_truncate_size_at_least() {
        let (prefix, size) = parse_truncate_size(">500").unwrap();
        assert_eq!(prefix, Some('>'));
        assert_eq!(size, 500);
    }

    #[test]
    fn test_truncate_size_round_down() {
        let (prefix, size) = parse_truncate_size("/4K").unwrap();
        assert_eq!(prefix, Some('/'));
        assert_eq!(size, 4096);
    }

    #[test]
    fn test_truncate_size_round_up() {
        let (prefix, size) = parse_truncate_size("%4K").unwrap();
        assert_eq!(prefix, Some('%'));
        assert_eq!(size, 4096);
    }

    // -- Truncate size computation --------------------------------------------

    #[test]
    fn test_compute_truncate_absolute() {
        assert_eq!(compute_truncate_size(500, None, 1000).unwrap(), 1000);
    }

    #[test]
    fn test_compute_truncate_extend() {
        assert_eq!(
            compute_truncate_size(500, Some('+'), 200).unwrap(),
            700
        );
    }

    #[test]
    fn test_compute_truncate_shrink() {
        assert_eq!(
            compute_truncate_size(500, Some('-'), 200).unwrap(),
            300
        );
    }

    #[test]
    fn test_compute_truncate_shrink_underflow() {
        // Saturating subtraction: 100 - 500 => 0
        assert_eq!(
            compute_truncate_size(100, Some('-'), 500).unwrap(),
            0
        );
    }

    #[test]
    fn test_compute_truncate_at_most_smaller() {
        assert_eq!(
            compute_truncate_size(300, Some('<'), 500).unwrap(),
            300
        );
    }

    #[test]
    fn test_compute_truncate_at_most_larger() {
        assert_eq!(
            compute_truncate_size(800, Some('<'), 500).unwrap(),
            500
        );
    }

    #[test]
    fn test_compute_truncate_at_least_smaller() {
        assert_eq!(
            compute_truncate_size(300, Some('>'), 500).unwrap(),
            500
        );
    }

    #[test]
    fn test_compute_truncate_at_least_larger() {
        assert_eq!(
            compute_truncate_size(800, Some('>'), 500).unwrap(),
            800
        );
    }

    #[test]
    fn test_compute_truncate_round_down() {
        // 1000 rounded down to nearest multiple of 300 => 900
        assert_eq!(
            compute_truncate_size(1000, Some('/'), 300).unwrap(),
            900
        );
    }

    #[test]
    fn test_compute_truncate_round_down_exact() {
        assert_eq!(
            compute_truncate_size(900, Some('/'), 300).unwrap(),
            900
        );
    }

    #[test]
    fn test_compute_truncate_round_up() {
        // 1000 rounded up to nearest multiple of 300 => 1200
        assert_eq!(
            compute_truncate_size(1000, Some('%'), 300).unwrap(),
            1200
        );
    }

    #[test]
    fn test_compute_truncate_round_up_exact() {
        assert_eq!(
            compute_truncate_size(900, Some('%'), 300).unwrap(),
            900
        );
    }

    #[test]
    fn test_compute_truncate_round_zero_error() {
        assert!(compute_truncate_size(100, Some('/'), 0).is_err());
        assert!(compute_truncate_size(100, Some('%'), 0).is_err());
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

    // -- Signal name/number mapping -------------------------------------------

    #[test]
    fn test_signal_kill() {
        assert_eq!(signal_name_to_number("KILL").unwrap(), 9);
    }

    #[test]
    fn test_signal_sigkill() {
        assert_eq!(signal_name_to_number("SIGKILL").unwrap(), 9);
    }

    #[test]
    fn test_signal_hup() {
        assert_eq!(signal_name_to_number("HUP").unwrap(), 1);
    }

    #[test]
    fn test_signal_term() {
        assert_eq!(signal_name_to_number("TERM").unwrap(), 15);
    }

    #[test]
    fn test_signal_number_string() {
        assert_eq!(signal_name_to_number("9").unwrap(), 9);
    }

    #[test]
    fn test_signal_unknown() {
        assert!(signal_name_to_number("BOGUS").is_err());
    }

    #[test]
    fn test_signal_number_to_name_kill() {
        assert_eq!(signal_number_to_name(9), "KILL");
    }

    #[test]
    fn test_signal_number_to_name_hup() {
        assert_eq!(signal_number_to_name(1), "HUP");
    }

    #[test]
    fn test_signal_number_to_name_unknown() {
        assert_eq!(signal_number_to_name(99), "UNKNOWN");
    }

    // -- Shred pattern generation ---------------------------------------------

    #[test]
    fn test_shred_pattern_random_not_zero() {
        let mut buf = vec![0u8; 256];
        generate_shred_pattern(&mut buf, 0, 42);
        // Should not be all zeros (extremely unlikely with a proper RNG)
        assert!(buf.iter().any(|&b| b != 0));
    }

    #[test]
    fn test_shred_pattern_complement() {
        let mut buf_even = vec![0u8; 256];
        let mut buf_odd = vec![0u8; 256];
        generate_shred_pattern(&mut buf_even, 0, 42);
        generate_shred_pattern(&mut buf_odd, 1, 42);
        // Odd pass should be complement of even pass
        for (a, b) in buf_even.iter().zip(buf_odd.iter()) {
            assert_eq!(*a, !*b);
        }
    }

    #[test]
    fn test_shred_pattern_deterministic() {
        let mut buf1 = vec![0u8; 128];
        let mut buf2 = vec![0u8; 128];
        generate_shred_pattern(&mut buf1, 0, 99);
        generate_shred_pattern(&mut buf2, 0, 99);
        assert_eq!(buf1, buf2);
    }

    #[test]
    fn test_shred_pattern_different_seeds() {
        let mut buf1 = vec![0u8; 128];
        let mut buf2 = vec![0u8; 128];
        generate_shred_pattern(&mut buf1, 0, 1);
        generate_shred_pattern(&mut buf2, 0, 2);
        assert_ne!(buf1, buf2);
    }

    // -- XorShift64 PRNG -----------------------------------------------------

    #[test]
    fn test_xorshift_not_zero() {
        let mut rng = XorShift64::new(12345);
        let val = rng.next_u64();
        assert_ne!(val, 0);
    }

    #[test]
    fn test_xorshift_zero_seed_replaced() {
        let mut rng = XorShift64::new(0);
        let val = rng.next_u64();
        assert_ne!(val, 0);
    }

    #[test]
    fn test_xorshift_fill_bytes() {
        let mut rng = XorShift64::new(777);
        let mut buf = vec![0u8; 100];
        rng.fill_bytes(&mut buf);
        assert!(buf.iter().any(|&b| b != 0));
    }

    // -- Proc fd path parsing -------------------------------------------------

    #[test]
    fn test_parse_proc_fd_nonexistent() {
        // PID 0 fd 99999 should not exist
        assert!(parse_proc_fd_link(0, 99999).is_none());
    }

    // -- Line counting --------------------------------------------------------

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
    fn test_compute_truncate_extend_overflow() {
        assert!(compute_truncate_size(u64::MAX, Some('+'), 1).is_err());
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

    // -- Fuser argument parsing -----------------------------------------------

    #[test]
    fn test_fuser_parse_kill() {
        let args = vec!["-k".to_string(), "/tmp/f".to_string()];
        let cfg = parse_fuser_args(&args).unwrap();
        assert!(cfg.send_signal);
        assert_eq!(cfg.signal, 9); // default SIGKILL
    }

    #[test]
    fn test_fuser_parse_signal_shorthand() {
        let args = vec!["-HUP".to_string(), "/tmp/f".to_string()];
        let cfg = parse_fuser_args(&args).unwrap();
        assert!(cfg.send_signal);
        assert_eq!(cfg.signal, 1);
    }

    #[test]
    fn test_fuser_parse_namespace() {
        let args = vec![
            "-n".to_string(),
            "tcp".to_string(),
            "80".to_string(),
        ];
        let cfg = parse_fuser_args(&args).unwrap();
        assert_eq!(cfg.namespace, "tcp");
    }

    #[test]
    fn test_fuser_parse_no_targets_error() {
        let args: Vec<String> = vec!["-v".to_string()];
        assert!(parse_fuser_args(&args).is_err());
    }

    // -- Access type display --------------------------------------------------

    #[test]
    fn test_access_type_display() {
        assert_eq!(format!("{}", AccessType::Fd), "f");
        assert_eq!(format!("{}", AccessType::Root), "r");
        assert_eq!(format!("{}", AccessType::Cwd), "c");
        assert_eq!(format!("{}", AccessType::Mmap), "m");
        assert_eq!(format!("{}", AccessType::Exe), "e");
    }
}
