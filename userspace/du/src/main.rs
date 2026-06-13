//! SlateOS Disk Usage Utility
//!
//! Estimates file and directory space usage by recursively walking directory
//! trees. Reports either disk usage (size rounded up to the SlateOS 16 KiB block
//! size) or apparent size (actual file bytes).
//!
//! # Usage
//!
//! ```text
//! du                              Show disk usage for current directory
//! du /home /tmp                   Show disk usage for multiple paths
//! du -h                           Human-readable sizes (1K=1024)
//! du --si                         Human-readable sizes (1K=1000)
//! du -s                           Summarize: only total per argument
//! du -a                           Show all files, not just directories
//! du -d 2                         Limit display depth to 2
//! du -c                           Show grand total at end
//! du -b                           Show apparent size in bytes
//! du -L                           Follow symbolic links
//! du -x                           Stay on one filesystem
//! du --exclude '*.o'              Exclude files matching glob
//! du --threshold 1M               Only show entries above 1 MiB
//! du --time                       Show last modification time
//! du --sort size                  Sort output by size
//! du --json                       JSON output
//! du --inodes                     Count inodes instead of bytes
//! ```

use std::collections::HashSet;
use std::env;
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process;
use std::time::SystemTime;

// ============================================================================
// Constants
// ============================================================================

/// SlateOS uses 16 KiB pages, so disk usage rounds up to this block size.
const BLOCK_SIZE: u64 = 16_384;

// ============================================================================
// CLI configuration
// ============================================================================

struct Config {
    /// Paths to measure (default: current directory).
    paths: Vec<PathBuf>,
    /// `-a` / `--all`: show usage for every file, not just directories.
    all_files: bool,
    /// `-s` / `--summarize`: show only the total for each argument.
    summarize: bool,
    /// `-h` / `--human-readable`: human sizes (1024-based).
    human_readable: bool,
    /// `--si`: human sizes (1000-based).
    si: bool,
    /// `-k`: show in KiB (the default).
    kilo: bool,
    /// `-m`: show in MiB.
    mega: bool,
    /// `-b` / `--bytes`: show apparent size in bytes.
    bytes: bool,
    /// `--apparent-size`: use file size instead of disk usage.
    apparent_size: bool,
    /// `-c` / `--total`: show grand total at end.
    total: bool,
    /// `-d <N>` / `--max-depth <N>`: maximum display depth.
    max_depth: Option<usize>,
    /// `-L` / `--dereference`: follow symlinks.
    dereference: bool,
    /// `-x` / `--one-file-system`: skip directories on different filesystems.
    one_file_system: bool,
    /// `--exclude <pattern>`: exclude files matching glob patterns.
    exclude_patterns: Vec<String>,
    /// `--threshold <size>`: only show entries above this byte count.
    threshold: Option<u64>,
    /// `--time`: show last modification time.
    show_time: bool,
    /// `--sort <field>`: sort output by "size" or "name".
    sort_by: Option<SortField>,
    /// `--json`: JSON output.
    json: bool,
    /// `--inodes`: count inodes instead of bytes.
    inodes: bool,
}

#[derive(Clone, Copy)]
enum SortField {
    Size,
    Name,
}

impl Config {
    fn new() -> Self {
        Self {
            paths: Vec::new(),
            all_files: false,
            summarize: false,
            human_readable: false,
            si: false,
            kilo: true,
            mega: false,
            bytes: false,
            apparent_size: false,
            total: false,
            max_depth: None,
            dereference: false,
            one_file_system: false,
            exclude_patterns: Vec::new(),
            threshold: None,
            show_time: false,
            sort_by: None,
            json: false,
            inodes: false,
        }
    }

    /// Whether to use apparent size (file bytes) rather than disk usage.
    fn use_apparent_size(&self) -> bool {
        self.apparent_size || self.bytes
    }
}

// ============================================================================
// Argument parsing
// ============================================================================

fn print_help() {
    let help = "\
Usage: du [OPTION]... [FILE]...

Summarize disk usage of the set of FILEs, recursively for directories.

Options:
  -a, --all              Write counts for all files, not just directories
  -s, --summarize        Display only a total for each argument
  -h, --human-readable   Print sizes in human-readable format (1K=1024)
      --si               Print sizes in SI format (1K=1000)
  -k                     Show sizes in 1K blocks (default)
  -m                     Show sizes in 1M blocks
  -b, --bytes            Show apparent size in bytes
      --apparent-size    Use apparent file sizes instead of disk usage
  -c, --total            Produce a grand total
  -d, --max-depth <N>    Print total for a directory only if it is N or fewer
                         levels below the command-line argument
  -L, --dereference      Follow all symbolic links
  -x, --one-file-system  Skip directories on different file systems
      --exclude <PAT>    Exclude files matching glob pattern
      --threshold <SIZE> Only show entries at least SIZE (e.g. 1M, 500K)
      --time             Show time of last modification
      --sort <FIELD>     Sort by 'size' or 'name'
      --json             Output in JSON format
      --inodes           Count inodes instead of disk usage
      --help             Display this help and exit
      --version          Output version information and exit";
    println!("{help}");
}

fn print_version() {
    println!("du (SlateOS coreutils) 0.1.0");
}

/// Parse a human-readable size string like "1M", "500K", "2G", "4096" into
/// bytes. Returns `None` on parse failure.
fn parse_size(s: &str) -> Option<u64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }

    let (num_part, suffix) = match s.bytes().rposition(|b| b.is_ascii_digit()) {
        Some(pos) => {
            let (n, sfx) = s.split_at(pos + 1);
            (n, sfx.trim())
        }
        None => return None,
    };

    let base: u64 = num_part.parse().ok()?;

    let multiplier = match suffix.to_ascii_uppercase().as_str() {
        "" | "B" => 1u64,
        "K" | "KB" | "KIB" => 1024,
        "M" | "MB" | "MIB" => 1024 * 1024,
        "G" | "GB" | "GIB" => 1024 * 1024 * 1024,
        "T" | "TB" | "TIB" => 1024 * 1024 * 1024 * 1024,
        _ => return None,
    };

    base.checked_mul(multiplier)
}

fn parse_args() -> Config {
    let mut cfg = Config::new();
    let args: Vec<String> = env::args().collect();
    let mut i = 1;

    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            "-a" | "--all" => cfg.all_files = true,
            "-s" | "--summarize" => cfg.summarize = true,
            "-h" | "--human-readable" => cfg.human_readable = true,
            "--si" => cfg.si = true,
            "-k" => {
                cfg.kilo = true;
                cfg.mega = false;
            }
            "-m" => {
                cfg.mega = true;
                cfg.kilo = false;
            }
            "-b" | "--bytes" => cfg.bytes = true,
            "--apparent-size" => cfg.apparent_size = true,
            "-c" | "--total" => cfg.total = true,
            "-d" | "--max-depth" => {
                i += 1;
                if i < args.len() {
                    match args[i].parse::<usize>() {
                        Ok(n) => cfg.max_depth = Some(n),
                        Err(_) => {
                            eprintln!("du: invalid maximum depth '{}'", args[i]);
                            process::exit(1);
                        }
                    }
                } else {
                    eprintln!("du: option '{arg}' requires an argument");
                    process::exit(1);
                }
            }
            "-L" | "--dereference" => cfg.dereference = true,
            "-x" | "--one-file-system" => cfg.one_file_system = true,
            "--exclude" => {
                i += 1;
                if i < args.len() {
                    cfg.exclude_patterns.push(args[i].clone());
                } else {
                    eprintln!("du: option '--exclude' requires an argument");
                    process::exit(1);
                }
            }
            "--threshold" => {
                i += 1;
                if i < args.len() {
                    match parse_size(&args[i]) {
                        Some(sz) => cfg.threshold = Some(sz),
                        None => {
                            eprintln!("du: invalid threshold '{}'", args[i]);
                            process::exit(1);
                        }
                    }
                } else {
                    eprintln!("du: option '--threshold' requires an argument");
                    process::exit(1);
                }
            }
            "--time" => cfg.show_time = true,
            "--sort" => {
                i += 1;
                if i < args.len() {
                    match args[i].as_str() {
                        "size" => cfg.sort_by = Some(SortField::Size),
                        "name" => cfg.sort_by = Some(SortField::Name),
                        other => {
                            eprintln!("du: invalid sort field '{other}' (use 'size' or 'name')");
                            process::exit(1);
                        }
                    }
                } else {
                    eprintln!("du: option '--sort' requires an argument");
                    process::exit(1);
                }
            }
            "--json" => cfg.json = true,
            "--inodes" => cfg.inodes = true,
            "--help" => {
                print_help();
                process::exit(0);
            }
            "--version" => {
                print_version();
                process::exit(0);
            }
            other if other.starts_with('-') && other.len() > 1 => {
                // Handle combined short flags like -sh, -ahk, etc.
                let chars: Vec<char> = other[1..].chars().collect();
                let mut j = 0;
                while j < chars.len() {
                    match chars[j] {
                        'a' => cfg.all_files = true,
                        's' => cfg.summarize = true,
                        'h' => cfg.human_readable = true,
                        'k' => {
                            cfg.kilo = true;
                            cfg.mega = false;
                        }
                        'm' => {
                            cfg.mega = true;
                            cfg.kilo = false;
                        }
                        'b' => cfg.bytes = true,
                        'c' => cfg.total = true,
                        'L' => cfg.dereference = true,
                        'x' => cfg.one_file_system = true,
                        'd' => {
                            // -d may be followed by the number inline or as
                            // next argument.
                            let rest: String = chars[j + 1..].iter().collect();
                            if !rest.is_empty() {
                                match rest.parse::<usize>() {
                                    Ok(n) => cfg.max_depth = Some(n),
                                    Err(_) => {
                                        eprintln!("du: invalid maximum depth '{rest}'");
                                        process::exit(1);
                                    }
                                }
                                // Consumed the rest of the combined flags.
                                j = chars.len();
                                continue;
                            }
                            i += 1;
                            if i < args.len() {
                                match args[i].parse::<usize>() {
                                    Ok(n) => cfg.max_depth = Some(n),
                                    Err(_) => {
                                        eprintln!("du: invalid maximum depth '{}'", args[i]);
                                        process::exit(1);
                                    }
                                }
                            } else {
                                eprintln!("du: option '-d' requires an argument");
                                process::exit(1);
                            }
                        }
                        unknown => {
                            eprintln!("du: unknown option '-{unknown}'");
                            eprintln!("Try 'du --help' for more information.");
                            process::exit(1);
                        }
                    }
                    j += 1;
                }
            }
            _ => cfg.paths.push(PathBuf::from(arg)),
        }
        i += 1;
    }

    // Default to current directory if no paths given.
    if cfg.paths.is_empty() {
        cfg.paths.push(PathBuf::from("."));
    }

    // `-b` implies `--apparent-size`.
    if cfg.bytes {
        cfg.apparent_size = true;
    }

    // `--si` overrides `-h` (last wins in GNU du, but we follow the spec).
    if cfg.si {
        cfg.human_readable = false;
    }

    cfg
}

// ============================================================================
// Glob pattern matching
// ============================================================================

/// Match a filename against a simple glob pattern.
///
/// Supports:
/// - `*` matches any sequence of characters (except `/`)
/// - `?` matches any single character (except `/`)
/// - Character classes like `[abc]` or `[a-z]`
/// - Everything else is a literal match.
fn glob_matches(pattern: &str, name: &str) -> bool {
    glob_match_inner(pattern.as_bytes(), name.as_bytes())
}

fn glob_match_inner(pat: &[u8], name: &[u8]) -> bool {
    let mut pi = 0;
    let mut ni = 0;
    // Positions to backtrack to on `*` failure.
    let mut star_pi: Option<usize> = None;
    let mut star_ni: usize = 0;

    while ni < name.len() {
        if pi < pat.len() && pat[pi] == b'?' && name[ni] != b'/' {
            pi += 1;
            ni += 1;
        } else if pi < pat.len() && pat[pi] == b'*' {
            // Record backtrack position.
            star_pi = Some(pi);
            star_ni = ni;
            pi += 1;
            // `*` may match zero characters initially.
        } else if pi < pat.len() && pat[pi] == b'[' {
            // Character class.
            if let Some((matched, end)) = match_char_class(&pat[pi..], name[ni]) {
                if matched {
                    pi += end;
                    ni += 1;
                } else if let Some(sp) = star_pi {
                    pi = sp + 1;
                    star_ni += 1;
                    ni = star_ni;
                } else {
                    return false;
                }
            } else {
                // Malformed class: treat `[` as literal.
                if pat[pi] == name[ni] {
                    pi += 1;
                    ni += 1;
                } else if let Some(sp) = star_pi {
                    pi = sp + 1;
                    star_ni += 1;
                    ni = star_ni;
                } else {
                    return false;
                }
            }
        } else if pi < pat.len()
            && pat[pi].eq_ignore_ascii_case(&name[ni])
            && pat[pi] != b'*'
            && pat[pi] != b'?'
        {
            pi += 1;
            ni += 1;
        } else if let Some(sp) = star_pi {
            // Backtrack: let `*` match one more character.
            pi = sp + 1;
            star_ni += 1;
            ni = star_ni;
        } else {
            return false;
        }
    }

    // Consume any trailing `*` in pattern.
    while pi < pat.len() && pat[pi] == b'*' {
        pi += 1;
    }

    pi == pat.len()
}

/// Try to match a character class like `[abc]` or `[a-z]`. Returns
/// `Some((matched, bytes_consumed))` or `None` if the class is malformed.
fn match_char_class(pat: &[u8], ch: u8) -> Option<(bool, usize)> {
    if pat.is_empty() || pat[0] != b'[' {
        return None;
    }

    let mut i = 1;
    let negate = if i < pat.len() && (pat[i] == b'!' || pat[i] == b'^') {
        i += 1;
        true
    } else {
        false
    };

    let mut matched = false;

    while i < pat.len() && pat[i] != b']' {
        if i + 2 < pat.len() && pat[i + 1] == b'-' && pat[i + 2] != b']' {
            // Range like a-z.
            let lo = pat[i];
            let hi = pat[i + 2];
            if ch >= lo && ch <= hi {
                matched = true;
            }
            i += 3;
        } else {
            if pat[i] == ch {
                matched = true;
            }
            i += 1;
        }
    }

    if i >= pat.len() {
        // No closing `]` found.
        return None;
    }

    // `i` is at `]`, so consume it.
    let consumed = i + 1;

    if negate {
        matched = !matched;
    }

    Some((matched, consumed))
}

/// Check whether a filename matches any of the exclude patterns.
fn is_excluded(name: &str, patterns: &[String]) -> bool {
    patterns.iter().any(|pat| glob_matches(pat, name))
}

// ============================================================================
// Formatting helpers
// ============================================================================

/// Format a byte count into human-readable form (powers of 1024).
fn human_readable(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "K", "M", "G", "T", "P"];
    if bytes == 0 {
        return "0".to_string();
    }
    let mut val = bytes as f64;
    for &unit in UNITS {
        if val < 1024.0 || unit == "P" {
            // GNU du -h shows one decimal place for scaled values below 10
            // (e.g. "1.0K", "1.5K") and no decimals at or above 10 ("512B",
            // "10K"). Do NOT special-case whole numbers — GNU prints "1.0K",
            // not "1K", for exactly 1024 bytes.
            return if val >= 10.0 {
                format!("{val:.0}{unit}")
            } else {
                format!("{val:.1}{unit}")
            };
        }
        val /= 1024.0;
    }
    format!("{bytes}")
}

/// Format a byte count into SI human-readable form (powers of 1000).
fn si_readable(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "kB", "MB", "GB", "TB", "PB"];
    if bytes == 0 {
        return "0".to_string();
    }
    let mut val = bytes as f64;
    for &unit in UNITS {
        if val < 1000.0 || unit == "PB" {
            // Match GNU du --si: one decimal below 10, none at or above.
            return if val >= 10.0 {
                format!("{val:.0}{unit}")
            } else {
                format!("{val:.1}{unit}")
            };
        }
        val /= 1000.0;
    }
    format!("{bytes}")
}

/// Format a size value according to the current config, returning a string.
fn format_size(bytes: u64, cfg: &Config) -> String {
    if cfg.bytes {
        return format!("{bytes}");
    }
    if cfg.human_readable {
        return human_readable(bytes);
    }
    if cfg.si {
        return si_readable(bytes);
    }
    if cfg.mega {
        let blocks = bytes.saturating_add(1_048_575) / 1_048_576;
        return format!("{blocks}");
    }
    // Default: KiB.
    let blocks = bytes.saturating_add(1023) / 1024;
    format!("{blocks}")
}

/// Format a `SystemTime` as a simple date-time string `YYYY-MM-DD HH:MM`.
///
/// Avoids pulling in chrono or time crates: computes the date from the Unix
/// epoch using basic arithmetic.
fn format_time(st: SystemTime) -> String {
    let secs = match st.duration_since(SystemTime::UNIX_EPOCH) {
        Ok(d) => d.as_secs(),
        Err(_) => return "?".to_string(),
    };

    let days = secs / 86400;
    let day_secs = secs % 86400;
    let hours = day_secs / 3600;
    let minutes = (day_secs % 3600) / 60;

    // Convert days since epoch (1970-01-01) to Y-M-D.
    let (year, month, day) = days_to_ymd(days);

    format!("{year:04}-{month:02}-{day:02} {hours:02}:{minutes:02}")
}

/// Convert days since 1970-01-01 to (year, month, day).
fn days_to_ymd(days: u64) -> (u64, u64, u64) {
    // Civil calendar algorithm from Howard Hinnant.
    let z = days as i64 + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64; // day of era [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };

    (y as u64, m, d)
}

// ============================================================================
// JSON helpers
// ============================================================================

/// Escape a string for JSON output.
fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => {
                for unit in c.encode_utf16(&mut [0u16; 2]) {
                    let _ = std::fmt::Write::write_fmt(&mut out, format_args!("\\u{unit:04x}"));
                }
            }
            c => out.push(c),
        }
    }
    out
}

// ============================================================================
// Metadata helpers
// ============================================================================

/// Portable inode/device extraction. On Unix-like systems (including SlateOS) we
/// use `std::os::unix::fs::MetadataExt`. On non-Unix build hosts we fall back
/// to stubs so the code still compiles for testing.
#[cfg(unix)]
mod meta_ext {
    use std::fs::Metadata;
    use std::os::unix::fs::MetadataExt;

    pub fn inode(m: &Metadata) -> u64 {
        m.ino()
    }

    pub fn device(m: &Metadata) -> u64 {
        m.dev()
    }

    pub fn file_size(m: &Metadata) -> u64 {
        m.size()
    }
}

#[cfg(not(unix))]
mod meta_ext {
    use std::fs::Metadata;

    /// Stub: no inode concept on non-Unix; return 0 (disables hardlink
    /// dedup, which is the safe default).
    pub fn inode(_m: &Metadata) -> u64 {
        0
    }

    /// Stub: return 0 (disables one-file-system check on non-Unix hosts).
    pub fn device(_m: &Metadata) -> u64 {
        0
    }

    /// File size from the standard `Metadata::len()`.
    pub fn file_size(m: &Metadata) -> u64 {
        m.len()
    }
}

/// Retrieve metadata for a path, following symlinks if `dereference` is set.
fn get_metadata(path: &Path, dereference: bool) -> std::io::Result<fs::Metadata> {
    if dereference {
        fs::metadata(path)
    } else {
        fs::symlink_metadata(path)
    }
}

/// Compute disk usage for a single file: apparent size rounded up to the 16 KiB
/// block boundary.
fn disk_usage(apparent: u64) -> u64 {
    if apparent == 0 {
        return 0;
    }
    // Round up to the next multiple of BLOCK_SIZE.
    apparent.saturating_add(BLOCK_SIZE - 1) / BLOCK_SIZE * BLOCK_SIZE
}

// ============================================================================
// Directory walk result
// ============================================================================

/// One entry in the walk output.
#[allow(dead_code)]
struct DuEntry {
    /// Path to display.
    path: String,
    /// Size in bytes (disk usage or apparent, depending on config).
    size: u64,
    /// Inode count (1 for files; accumulated for directories).
    inode_count: u64,
    /// Last modification time, if available.
    mtime: Option<SystemTime>,
    /// Whether this is a directory entry.
    is_dir: bool,
    /// Depth relative to the command-line argument root (0 = the argument).
    depth: usize,
}

// ============================================================================
// Recursive directory walk
// ============================================================================

/// Walk a directory tree rooted at `root`, collecting `DuEntry` items.
///
/// Returns the total size of the tree rooted at `root` (in the chosen metric)
/// and pushes per-entry results into `results`.
fn walk(
    root: &Path,
    depth: usize,
    cfg: &Config,
    visited_inodes: &mut HashSet<(u64, u64)>,
    root_device: Option<u64>,
    results: &mut Vec<DuEntry>,
    exit_code: &mut i32,
) -> (u64, u64) {
    let meta = match get_metadata(root, cfg.dereference) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("du: cannot access '{}': {e}", root.display());
            *exit_code = 1;
            return (0, 0);
        }
    };

    let ino = meta_ext::inode(&meta);
    let dev = meta_ext::device(&meta);

    // Check one-file-system: skip if device differs from root.
    if cfg.one_file_system
        && let Some(rd) = root_device
            && dev != rd {
                return (0, 0);
            }

    // Determine the effective root device (set on first call).
    let effective_root_device = root_device.or(Some(dev));

    // Detect hard links: if we have already seen this (device, inode) pair,
    // skip it to avoid double-counting. Inode 0 means the platform does not
    // support inodes, so we skip the check.
    if ino != 0 && !visited_inodes.insert((dev, ino)) {
        return (0, 0);
    }

    if meta.is_file() || meta.is_symlink() {
        let apparent = meta_ext::file_size(&meta);
        let size = if cfg.use_apparent_size() {
            apparent
        } else {
            disk_usage(apparent)
        };
        let inode_count = 1u64;

        return (size, inode_count);
    }

    if !meta.is_dir() {
        // Special file (socket, device, etc.): count zero size.
        return (0, 1);
    }

    // It is a directory. Walk its children.
    let entries = match fs::read_dir(root) {
        Ok(rd) => rd,
        Err(e) => {
            eprintln!("du: cannot read directory '{}': {e}", root.display());
            *exit_code = 1;
            return (0, 1);
        }
    };

    let mut total_size: u64 = 0;
    let mut total_inodes: u64 = 1; // count the directory itself
    let mut child_entries: Vec<DuEntry> = Vec::new();

    for entry_result in entries {
        let entry = match entry_result {
            Ok(e) => e,
            Err(e) => {
                eprintln!("du: error reading entry in '{}': {e}", root.display());
                *exit_code = 1;
                continue;
            }
        };

        let file_name = entry.file_name();
        let name_str = file_name.to_string_lossy();

        // Check exclusion patterns against the filename.
        if is_excluded(&name_str, &cfg.exclude_patterns) {
            continue;
        }

        let child_path = root.join(&file_name);

        let (child_size, child_inodes) = walk(
            &child_path,
            depth + 1,
            cfg,
            visited_inodes,
            effective_root_device,
            &mut child_entries,
            exit_code,
        );

        total_size = total_size.saturating_add(child_size);
        total_inodes = total_inodes.saturating_add(child_inodes);

        // Decide whether to record this child in results.
        let child_meta = get_metadata(&child_path, cfg.dereference).ok();
        let child_is_dir = child_meta.as_ref().is_some_and(|m| m.is_dir());
        let child_mtime = child_meta.as_ref().and_then(|m| m.modified().ok());

        let display_size = if cfg.inodes { child_inodes } else { child_size };

        // In summarize mode we only show the top-level total.
        if !cfg.summarize {
            let within_depth = cfg.max_depth.is_none_or(|md| depth < md);

            if within_depth && (cfg.all_files || child_is_dir) && child_size > 0 {
                child_entries.push(DuEntry {
                    path: child_path.to_string_lossy().into_owned(),
                    size: display_size,
                    inode_count: child_inodes,
                    mtime: child_mtime,
                    is_dir: child_is_dir,
                    depth: depth + 1,
                });
            }
        }
    }

    // Append child entries to results.
    results.append(&mut child_entries);

    (total_size, total_inodes)
}

// ============================================================================
// Output
// ============================================================================

/// Print results in the standard tabular format.
fn print_results(entries: &[DuEntry], cfg: &Config) {
    let stdout = std::io::stdout();
    let mut out = std::io::BufWriter::new(stdout.lock());

    for entry in entries {
        let size_str = if cfg.inodes {
            format!("{}", entry.inode_count)
        } else {
            format_size(entry.size, cfg)
        };

        if let Some(threshold) = cfg.threshold {
            let effective = if cfg.inodes {
                entry.inode_count
            } else {
                entry.size
            };
            if effective < threshold {
                continue;
            }
        }

        if cfg.show_time {
            let time_str = entry
                .mtime
                .map(format_time)
                .unwrap_or_else(|| "?".to_string());
            let _ = writeln!(out, "{size_str}\t{time_str}\t{}", entry.path);
        } else {
            let _ = writeln!(out, "{size_str}\t{}", entry.path);
        }
    }
}

/// Print results in JSON format.
fn print_json(entries: &[DuEntry], cfg: &Config) {
    let stdout = std::io::stdout();
    let mut out = std::io::BufWriter::new(stdout.lock());

    let _ = writeln!(out, "[");
    let last = entries.len().saturating_sub(1);
    for (idx, entry) in entries.iter().enumerate() {
        let comma = if idx < last { "," } else { "" };
        let path = json_escape(&entry.path);

        if cfg.inodes {
            let _ = writeln!(
                out,
                "  {{\"path\":\"{path}\",\"inodes\":{}}}{comma}",
                entry.inode_count,
            );
        } else {
            let size = entry.size;
            if cfg.show_time {
                let time_str = entry
                    .mtime
                    .map(format_time)
                    .unwrap_or_else(|| "?".to_string());
                let time_escaped = json_escape(&time_str);
                let _ = writeln!(
                    out,
                    "  {{\"path\":\"{path}\",\"size\":{size},\"time\":\"{time_escaped}\"}}{comma}",
                );
            } else {
                let _ = writeln!(out, "  {{\"path\":\"{path}\",\"size\":{size}}}{comma}",);
            }
        }
    }
    let _ = writeln!(out, "]");
}

// ============================================================================
// Entry point
// ============================================================================

fn run() -> i32 {
    let cfg = parse_args();
    let mut exit_code = 0;
    let mut all_entries: Vec<DuEntry> = Vec::new();
    let mut grand_total: u64 = 0;
    let mut grand_inodes: u64 = 0;

    for root_path in &cfg.paths {
        let mut visited_inodes: HashSet<(u64, u64)> = HashSet::new();
        let mut results: Vec<DuEntry> = Vec::new();

        let (total_size, total_inodes) = walk(
            root_path,
            0,
            &cfg,
            &mut visited_inodes,
            None,
            &mut results,
            &mut exit_code,
        );

        grand_total = grand_total.saturating_add(total_size);
        grand_inodes = grand_inodes.saturating_add(total_inodes);

        let display_size = if cfg.inodes { total_inodes } else { total_size };

        // Get mtime for the root path itself.
        let root_mtime = get_metadata(root_path, cfg.dereference)
            .ok()
            .and_then(|m| m.modified().ok());

        // The root entry (depth 0) is always within any max_depth, since
        // max_depth is usize (>= 0). Always show the per-argument total.
        results.push(DuEntry {
            path: root_path.to_string_lossy().into_owned(),
            size: display_size,
            inode_count: total_inodes,
            mtime: root_mtime,
            is_dir: true,
            depth: 0,
        });

        // Sort if requested.
        if let Some(field) = cfg.sort_by {
            match field {
                SortField::Size => results.sort_by(|a, b| {
                    let sa = if cfg.inodes { a.inode_count } else { a.size };
                    let sb = if cfg.inodes { b.inode_count } else { b.size };
                    sa.cmp(&sb)
                }),
                SortField::Name => results.sort_by(|a, b| a.path.cmp(&b.path)),
            }
        }

        all_entries.append(&mut results);
    }

    // Grand total row (with -c / --total).
    if cfg.total {
        let display_total = if cfg.inodes {
            grand_inodes
        } else {
            grand_total
        };
        all_entries.push(DuEntry {
            path: "total".to_string(),
            size: display_total,
            inode_count: grand_inodes,
            mtime: None,
            is_dir: true,
            depth: 0,
        });
    }

    if cfg.json {
        print_json(&all_entries, &cfg);
    } else {
        print_results(&all_entries, &cfg);
    }

    exit_code
}

fn main() {
    process::exit(run());
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs as stdfs;

    // -- Glob matching tests --

    #[test]
    fn glob_star_matches_any() {
        assert!(glob_matches("*.rs", "main.rs"));
        assert!(glob_matches("*.rs", ".rs"));
        assert!(!glob_matches("*.rs", "main.txt"));
    }

    #[test]
    fn glob_question_mark() {
        assert!(glob_matches("?.rs", "a.rs"));
        assert!(!glob_matches("?.rs", "ab.rs"));
    }

    #[test]
    fn glob_char_class() {
        assert!(glob_matches("[abc].txt", "a.txt"));
        assert!(glob_matches("[abc].txt", "b.txt"));
        assert!(!glob_matches("[abc].txt", "d.txt"));
    }

    #[test]
    fn glob_char_range() {
        assert!(glob_matches("[a-z].txt", "m.txt"));
        assert!(!glob_matches("[a-z].txt", "1.txt"));
    }

    #[test]
    fn glob_negated_class() {
        assert!(!glob_matches("[!abc].txt", "a.txt"));
        assert!(glob_matches("[!abc].txt", "d.txt"));
    }

    #[test]
    fn glob_literal_match() {
        assert!(glob_matches("exact", "exact"));
        assert!(!glob_matches("exact", "other"));
    }

    #[test]
    fn glob_star_prefix() {
        assert!(glob_matches("test*", "testing"));
        assert!(glob_matches("test*", "test"));
    }

    #[test]
    fn glob_empty_pattern_matches_empty() {
        assert!(glob_matches("", ""));
        assert!(!glob_matches("", "x"));
    }

    #[test]
    fn glob_star_only() {
        assert!(glob_matches("*", "anything"));
        assert!(glob_matches("*", ""));
    }

    // -- Size parsing tests --

    #[test]
    fn parse_size_bytes() {
        assert_eq!(parse_size("4096"), Some(4096));
        assert_eq!(parse_size("0"), Some(0));
    }

    #[test]
    fn parse_size_kilo() {
        assert_eq!(parse_size("1K"), Some(1024));
        assert_eq!(parse_size("2KB"), Some(2048));
    }

    #[test]
    fn parse_size_mega() {
        assert_eq!(parse_size("1M"), Some(1_048_576));
        assert_eq!(parse_size("5MB"), Some(5_242_880));
    }

    #[test]
    fn parse_size_giga() {
        assert_eq!(parse_size("1G"), Some(1_073_741_824));
    }

    #[test]
    fn parse_size_invalid() {
        assert_eq!(parse_size(""), None);
        assert_eq!(parse_size("abc"), None);
        assert_eq!(parse_size("1X"), None);
    }

    // -- Disk usage rounding tests --

    #[test]
    fn disk_usage_zero() {
        assert_eq!(disk_usage(0), 0);
    }

    #[test]
    fn disk_usage_exact_block() {
        assert_eq!(disk_usage(BLOCK_SIZE), BLOCK_SIZE);
    }

    #[test]
    fn disk_usage_rounds_up() {
        assert_eq!(disk_usage(1), BLOCK_SIZE);
        assert_eq!(disk_usage(BLOCK_SIZE - 1), BLOCK_SIZE);
        assert_eq!(disk_usage(BLOCK_SIZE + 1), 2 * BLOCK_SIZE);
    }

    // -- Formatting tests --

    #[test]
    fn human_readable_small() {
        assert_eq!(human_readable(0), "0");
        assert_eq!(human_readable(512), "512B");
    }

    #[test]
    fn human_readable_kilo() {
        assert_eq!(human_readable(1024), "1.0K");
        assert_eq!(human_readable(1536), "1.5K");
    }

    #[test]
    fn human_readable_mega() {
        assert_eq!(human_readable(1_048_576), "1.0M");
    }

    #[test]
    fn si_readable_small() {
        assert_eq!(si_readable(0), "0");
        assert_eq!(si_readable(999), "999B");
    }

    #[test]
    fn si_readable_kilo() {
        assert_eq!(si_readable(1000), "1.0kB");
    }

    // -- format_size tests --

    #[test]
    fn format_size_default_kib() {
        let cfg = Config::new();
        // 2048 bytes = 2 KiB blocks
        assert_eq!(format_size(2048, &cfg), "2");
    }

    #[test]
    fn format_size_mib() {
        let mut cfg = Config::new();
        cfg.mega = true;
        cfg.kilo = false;
        // 2 MiB = 2 * 1048576
        assert_eq!(format_size(2_097_152, &cfg), "2");
    }

    #[test]
    fn format_size_bytes_mode() {
        let mut cfg = Config::new();
        cfg.bytes = true;
        cfg.apparent_size = true;
        assert_eq!(format_size(12345, &cfg), "12345");
    }

    // -- Date conversion test --

    #[test]
    fn days_to_ymd_epoch() {
        assert_eq!(days_to_ymd(0), (1970, 1, 1));
    }

    #[test]
    fn days_to_ymd_known_date() {
        // 2024-01-01 is 19723 days after epoch.
        assert_eq!(days_to_ymd(19723), (2024, 1, 1));
    }

    // -- Exclusion tests --

    #[test]
    fn exclude_matches() {
        let patterns = vec!["*.o".to_string(), "*.tmp".to_string()];
        assert!(is_excluded("foo.o", &patterns));
        assert!(is_excluded("bar.tmp", &patterns));
        assert!(!is_excluded("main.rs", &patterns));
    }

    // -- JSON escape tests --

    #[test]
    fn json_escape_simple() {
        assert_eq!(json_escape("hello"), "hello");
    }

    #[test]
    fn json_escape_special() {
        assert_eq!(json_escape("a\"b"), "a\\\"b");
        assert_eq!(json_escape("a\\b"), "a\\\\b");
        assert_eq!(json_escape("a\nb"), "a\\nb");
    }

    // -- Integration-style test using a temp directory --

    #[test]
    fn walk_temp_dir() {
        let dir = std::env::temp_dir().join("du_test_walk");
        let _ = stdfs::remove_dir_all(&dir);
        stdfs::create_dir_all(dir.join("sub")).expect("create dirs");

        // Write some data.
        stdfs::write(dir.join("file1.txt"), "hello").expect("write file1");
        stdfs::write(dir.join("sub/file2.txt"), "world!").expect("write file2");

        let cfg = Config::new();
        let mut visited = HashSet::new();
        let mut results = Vec::new();
        let mut exit_code = 0;

        let (total, _inodes) = walk(
            &dir,
            0,
            &cfg,
            &mut visited,
            None,
            &mut results,
            &mut exit_code,
        );

        assert_eq!(exit_code, 0);
        // Both files are small, so each takes one block = 16384.
        // Total should be 2 * 16384 = 32768.
        assert_eq!(total, 2 * BLOCK_SIZE);

        // Clean up.
        let _ = stdfs::remove_dir_all(&dir);
    }

    #[test]
    fn walk_temp_dir_apparent_size() {
        let dir = std::env::temp_dir().join("du_test_apparent");
        let _ = stdfs::remove_dir_all(&dir);
        stdfs::create_dir_all(&dir).expect("create dir");

        stdfs::write(dir.join("a.txt"), "12345").expect("write");

        let mut cfg = Config::new();
        cfg.apparent_size = true;

        let mut visited = HashSet::new();
        let mut results = Vec::new();
        let mut exit_code = 0;

        let (total, _) = walk(
            &dir,
            0,
            &cfg,
            &mut visited,
            None,
            &mut results,
            &mut exit_code,
        );

        assert_eq!(exit_code, 0);
        assert_eq!(total, 5); // "12345" is 5 bytes.

        let _ = stdfs::remove_dir_all(&dir);
    }

    #[test]
    fn walk_with_exclude() {
        let dir = std::env::temp_dir().join("du_test_exclude");
        let _ = stdfs::remove_dir_all(&dir);
        stdfs::create_dir_all(&dir).expect("create dir");

        stdfs::write(dir.join("keep.txt"), "data").expect("write keep");
        stdfs::write(dir.join("skip.tmp"), "data").expect("write skip");

        let mut cfg = Config::new();
        cfg.exclude_patterns.push("*.tmp".to_string());

        let mut visited = HashSet::new();
        let mut results = Vec::new();
        let mut exit_code = 0;

        let (total, _) = walk(
            &dir,
            0,
            &cfg,
            &mut visited,
            None,
            &mut results,
            &mut exit_code,
        );

        assert_eq!(exit_code, 0);
        // Only keep.txt should be counted: one block.
        assert_eq!(total, BLOCK_SIZE);

        let _ = stdfs::remove_dir_all(&dir);
    }
}
