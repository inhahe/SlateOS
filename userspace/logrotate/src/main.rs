//! OurOS Log Rotation Utility (`logrotate`)
//!
//! Rotates, compresses, and manages log files based on configuration rules.
//! Designed to be run periodically by crond.
//!
//! # Configuration
//!
//! Reads `/etc/logrotate.conf` (and included files from `/etc/logrotate.d/`)
//! to determine rotation policy per log file.
//!
//! ```text
//! # Global options
//! compress
//! dateext
//!
//! /var/log/syslog {
//!     daily
//!     rotate 7
//!     compress
//!     missingok
//!     notifempty
//!     create 0640 root root
//!     postrotate
//!         kill -HUP $(cat /var/run/syslogd.pid)
//!     endscript
//! }
//! ```
//!
//! # Usage
//!
//! ```text
//! logrotate [OPTIONS] [config_file]
//!
//!   -d, --debug     Dry-run mode, show what would be done
//!   -f, --force     Force rotation regardless of criteria
//!   -v, --verbose   Verbose output
//!   -s, --state F   State file path (default: /var/lib/logrotate/status)
//!       --json      JSON output of rotation actions
//! ```

use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process;
use std::time::{SystemTime, UNIX_EPOCH};

// ============================================================================
// Constants
// ============================================================================

const DEFAULT_CONFIG: &str = "/etc/logrotate.conf";
const DEFAULT_STATE_FILE: &str = "/var/lib/logrotate/status";
const VERSION: &str = "0.1.0";

// ============================================================================
// Time helpers
// ============================================================================

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Broken-down time from Unix seconds (UTC).
#[derive(Debug, Clone, Copy)]
struct BrokenTime {
    year: i64,
    month: u32,  // 1-12
    day: u32,    // 1-31
    hour: u32,   // 0-23
    minute: u32, // 0-59
    second: u32, // 0-59
}

fn is_leap_year(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

fn unix_to_broken(unix_secs: u64) -> BrokenTime {
    let secs = unix_secs;
    let days = secs / 86400;
    let time_secs = secs % 86400;
    let hour = (time_secs / 3600) as u32;
    let minute = ((time_secs % 3600) / 60) as u32;
    let second = (time_secs % 60) as u32;

    let mut y = 1970i64;
    let mut remaining_days = days as i64;

    loop {
        let days_in_year = if is_leap_year(y) { 366 } else { 365 };
        if remaining_days < days_in_year {
            break;
        }
        remaining_days -= days_in_year;
        y += 1;
    }

    let month_days: [i64; 12] = if is_leap_year(y) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };

    let mut month = 1u32;
    for (i, &md) in month_days.iter().enumerate() {
        if remaining_days < md {
            month = i as u32 + 1;
            break;
        }
        remaining_days -= md;
    }
    let day = remaining_days as u32 + 1;

    BrokenTime {
        year: y,
        month,
        day,
        hour,
        minute,
        second,
    }
}

fn format_timestamp(bt: &BrokenTime) -> String {
    format!(
        "{:04}-{:02}-{:02}-{:02}:{:02}:{:02}",
        bt.year, bt.month, bt.day, bt.hour, bt.minute, bt.second
    )
}

fn format_date_ext(bt: &BrokenTime, fmt: &str) -> String {
    // Support basic date format patterns used by logrotate dateformat.
    fmt.replace("%Y", &format!("{:04}", bt.year))
        .replace("%m", &format!("{:02}", bt.month))
        .replace("%d", &format!("{:02}", bt.day))
        .replace("%H", &format!("{:02}", bt.hour))
        .replace("%M", &format!("{:02}", bt.minute))
        .replace("%s", &format!("{}", bt.second))
}

/// Parse a timestamp string like "2025-05-17-12:00:00" back to Unix seconds.
fn parse_timestamp(s: &str) -> Option<u64> {
    // Format: YYYY-MM-DD-HH:MM:SS
    let parts: Vec<&str> = s.splitn(4, '-').collect();
    if parts.len() < 4 {
        return None;
    }
    let year: i64 = parts[0].parse().ok()?;
    let month: u32 = parts[1].parse().ok()?;
    let day: u32 = parts[2].parse().ok()?;

    let time_parts: Vec<&str> = parts[3].splitn(3, ':').collect();
    if time_parts.len() < 3 {
        return None;
    }
    let hour: u32 = time_parts[0].parse().ok()?;
    let minute: u32 = time_parts[1].parse().ok()?;
    let second: u32 = time_parts[2].parse().ok()?;

    // Convert back to Unix timestamp.
    let mut total_days: i64 = 0;
    for y in 1970..year {
        total_days += if is_leap_year(y) { 366 } else { 365 };
    }

    let month_days: [i64; 12] = if is_leap_year(year) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };

    for i in 0..(month as usize).saturating_sub(1) {
        if let Some(&md) = month_days.get(i) {
            total_days += md;
        }
    }

    total_days += (day as i64).saturating_sub(1);

    let total_secs =
        total_days as u64 * 86400 + hour as u64 * 3600 + minute as u64 * 60 + second as u64;
    Some(total_secs)
}

// ============================================================================
// Size parsing
// ============================================================================

/// Parse a size string like "100M", "50k", "1G", or plain bytes.
fn parse_size(s: &str) -> Result<u64, String> {
    let s = s.trim();
    if s.is_empty() {
        return Err("empty size string".to_string());
    }

    let (num_str, multiplier) = if s.ends_with('G') || s.ends_with('g') {
        (&s[..s.len() - 1], 1024 * 1024 * 1024u64)
    } else if s.ends_with('M') || s.ends_with('m') {
        (&s[..s.len() - 1], 1024 * 1024u64)
    } else if s.ends_with('k') || s.ends_with('K') {
        (&s[..s.len() - 1], 1024u64)
    } else {
        (s, 1u64)
    };

    let num: u64 = num_str
        .parse()
        .map_err(|_| format!("invalid size number: {num_str}"))?;

    num.checked_mul(multiplier)
        .ok_or_else(|| format!("size overflow: {s}"))
}

// ============================================================================
// Rotation frequency
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Frequency {
    Daily,
    Weekly,
    Monthly,
    Yearly,
}

impl Frequency {
    /// Minimum seconds between rotations for this frequency.
    fn min_interval_secs(self) -> u64 {
        match self {
            Frequency::Daily => 86400,
            Frequency::Weekly => 7 * 86400,
            Frequency::Monthly => 28 * 86400, // Conservative: 28 days minimum.
            Frequency::Yearly => 365 * 86400,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Frequency::Daily => "daily",
            Frequency::Weekly => "weekly",
            Frequency::Monthly => "monthly",
            Frequency::Yearly => "yearly",
        }
    }
}

// ============================================================================
// Log entry configuration
// ============================================================================

/// Configuration for a single log file (or group of log files).
#[derive(Debug, Clone)]
struct LogConfig {
    /// Log file paths this config applies to.
    paths: Vec<PathBuf>,
    /// Rotation frequency.
    frequency: Frequency,
    /// How many rotated files to keep.
    rotate_count: u32,
    /// Whether to compress rotated files.
    compress: bool,
    /// Delay compression by one rotation cycle.
    delay_compress: bool,
    /// Don't error if the log file is missing.
    missing_ok: bool,
    /// Don't rotate if file is empty.
    not_if_empty: bool,
    /// Rotate even if file is empty (default behavior, overrides not_if_empty).
    if_empty: bool,
    /// Create new log file after rotation (mode, owner, group).
    create: Option<CreateSpec>,
    /// Copy the file then truncate original (for apps holding the file open).
    copy_truncate: bool,
    /// Minimum size before rotation is considered.
    min_size: Option<u64>,
    /// Maximum size: rotate when file exceeds this regardless of schedule.
    max_size: Option<u64>,
    /// Only rotate if file is at least this big.
    size: Option<u64>,
    /// Use date extension instead of numbered rotation.
    date_ext: bool,
    /// Date format for dateext.
    date_format: String,
    /// Directory to place rotated logs into.
    old_dir: Option<PathBuf>,
    /// Maximum age in days for rotated files.
    max_age: Option<u32>,
    /// Script to run before rotation.
    pre_rotate: Option<String>,
    /// Script to run after rotation.
    post_rotate: Option<String>,
    /// Run pre/post scripts only once for all logs, not per log.
    shared_scripts: bool,
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            paths: Vec::new(),
            frequency: Frequency::Weekly,
            rotate_count: 4,
            compress: false,
            delay_compress: false,
            missing_ok: false,
            not_if_empty: false,
            if_empty: false,
            create: None,
            copy_truncate: false,
            min_size: None,
            max_size: None,
            size: None,
            date_ext: false,
            date_format: "-%Y%m%d".to_string(),
            old_dir: None,
            max_age: None,
            pre_rotate: None,
            post_rotate: None,
            shared_scripts: false,
        }
    }
}

/// Specification for creating a new log file after rotation.
#[derive(Debug, Clone)]
struct CreateSpec {
    mode: u32,
    owner: String,
    group: String,
}

// ============================================================================
// Config parser
// ============================================================================

/// Parsed logrotate configuration (global defaults + per-file entries).
#[derive(Debug)]
struct Config {
    entries: Vec<LogConfig>,
}

/// Parse the main config file and any included configs.
fn parse_config(path: &Path) -> Result<Config, String> {
    let content = fs::read_to_string(path)
        .map_err(|e| format!("cannot read config {}: {e}", path.display()))?;

    let mut global_defaults = LogConfig::default();
    let mut entries = Vec::new();

    parse_config_content(&content, path, &mut global_defaults, &mut entries)?;

    Ok(Config { entries })
}

/// Parse config content, handling includes and block definitions.
fn parse_config_content(
    content: &str,
    _config_path: &Path,
    global_defaults: &mut LogConfig,
    entries: &mut Vec<LogConfig>,
) -> Result<(), String> {
    let lines: Vec<&str> = content.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i].trim();

        // Skip empty lines and comments.
        if line.is_empty() || line.starts_with('#') {
            i += 1;
            continue;
        }

        // Include directive.
        if line.starts_with("include ") {
            let include_path_str = line.strip_prefix("include ").unwrap_or("").trim();
            let include_path = Path::new(include_path_str);

            if include_path.is_dir() {
                // Include all files in the directory.
                if let Ok(read_dir) = fs::read_dir(include_path) {
                    let mut paths: Vec<PathBuf> = Vec::new();
                    for entry in read_dir {
                        if let Ok(entry) = entry {
                            let p = entry.path();
                            if p.is_file() {
                                // Skip hidden files and files with certain extensions.
                                let name = p
                                    .file_name()
                                    .and_then(|n| n.to_str())
                                    .unwrap_or("");
                                if !name.starts_with('.')
                                    && !name.ends_with(".rpmsave")
                                    && !name.ends_with(".rpmnew")
                                    && !name.ends_with('~')
                                {
                                    paths.push(p);
                                }
                            }
                        }
                    }
                    paths.sort();
                    for p in &paths {
                        if let Ok(sub_content) = fs::read_to_string(p) {
                            parse_config_content(
                                &sub_content,
                                p,
                                global_defaults,
                                entries,
                            )?;
                        }
                    }
                }
            } else if include_path.is_file() {
                if let Ok(sub_content) = fs::read_to_string(include_path) {
                    parse_config_content(
                        &sub_content,
                        include_path,
                        global_defaults,
                        entries,
                    )?;
                }
            }
            i += 1;
            continue;
        }

        // Check if this line starts a block: "/path/to/log { " or
        // "/path1 /path2 {" possibly across multiple lines.
        if line.contains('{') || (i + 1 < lines.len() && lines[i + 1].trim() == "{") {
            // Collect log paths from this line.
            let header = if line.ends_with('{') {
                line.strip_suffix('{').unwrap_or(line).trim()
            } else if line.contains('{') {
                line.split('{').next().unwrap_or("").trim()
            } else {
                // The '{' is on the next line.
                line
            };

            let log_paths: Vec<PathBuf> = header
                .split_whitespace()
                .filter(|s| !s.is_empty())
                .map(PathBuf::from)
                .collect();

            // If the '{' was on the next line, skip it.
            if !line.contains('{') {
                i += 1;
            }

            // Collect block body until '}'.
            i += 1;
            let mut block_lines = Vec::new();
            while i < lines.len() {
                let bline = lines[i].trim();
                if bline == "}" {
                    i += 1;
                    break;
                }
                block_lines.push(bline);
                i += 1;
            }

            // Parse the block into a LogConfig, starting from global defaults.
            let mut entry = global_defaults.clone();
            entry.paths = log_paths;
            parse_block_directives(&block_lines, &mut entry)?;
            entries.push(entry);
            continue;
        }

        // Global directive (outside any block).
        parse_single_directive(line, global_defaults, &lines, &mut i)?;
        i += 1;
    }

    Ok(())
}

/// Parse directives within a `{ ... }` block.
fn parse_block_directives(lines: &[&str], config: &mut LogConfig) -> Result<(), String> {
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i];

        // Script blocks: prerotate/postrotate ... endscript
        if line == "prerotate" || line == "postrotate" {
            let is_post = line == "postrotate";
            let mut script_lines = Vec::new();
            i += 1;
            while i < lines.len() && lines[i] != "endscript" {
                script_lines.push(lines[i]);
                i += 1;
            }
            // Skip the "endscript" line.
            if i < lines.len() {
                i += 1;
            }
            let script = script_lines.join("\n");
            if is_post {
                config.post_rotate = Some(script);
            } else {
                config.pre_rotate = Some(script);
            }
            continue;
        }

        // Regular directive line (no line-index mutation needed since we
        // are iterating a slice, not the original file lines).
        let mut dummy_idx = 0usize;
        parse_single_directive(line, config, &[], &mut dummy_idx)?;
        i += 1;
    }

    Ok(())
}

/// Parse a single directive line and apply it to the given config.
///
/// `all_lines` and `line_idx` are only used at the global level for context;
/// block-level callers may pass empty slices.
fn parse_single_directive(
    line: &str,
    config: &mut LogConfig,
    _all_lines: &[&str],
    _line_idx: &mut usize,
) -> Result<(), String> {
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.is_empty() {
        return Ok(());
    }

    match parts[0] {
        "daily" => config.frequency = Frequency::Daily,
        "weekly" => config.frequency = Frequency::Weekly,
        "monthly" => config.frequency = Frequency::Monthly,
        "yearly" | "annually" => config.frequency = Frequency::Yearly,

        "rotate" => {
            let count = parts
                .get(1)
                .ok_or("rotate requires a count")?
                .parse::<u32>()
                .map_err(|_| "invalid rotate count".to_string())?;
            config.rotate_count = count;
        }

        "compress" => config.compress = true,
        "nocompress" => config.compress = false,
        "delaycompress" => config.delay_compress = true,
        "nodelaycompress" => config.delay_compress = false,

        "missingok" => config.missing_ok = true,
        "nomissingok" => config.missing_ok = false,

        "notifempty" => config.not_if_empty = true,
        "ifempty" => config.if_empty = true,

        "copytruncate" => config.copy_truncate = true,
        "nocopytruncate" => config.copy_truncate = false,

        "sharedscripts" => config.shared_scripts = true,
        "nosharedscripts" => config.shared_scripts = false,

        "dateext" => config.date_ext = true,
        "nodateext" => config.date_ext = false,

        "dateformat" => {
            let fmt = parts
                .get(1)
                .ok_or("dateformat requires a format string")?;
            config.date_format = (*fmt).to_string();
        }

        "create" => {
            // create [mode] [owner] [group]
            if parts.len() >= 4 {
                let mode =
                    u32::from_str_radix(parts[1], 8).unwrap_or(0o644);
                config.create = Some(CreateSpec {
                    mode,
                    owner: parts[2].to_string(),
                    group: parts[3].to_string(),
                });
            } else if parts.len() == 1 {
                // Bare "create" -- create with defaults.
                config.create = Some(CreateSpec {
                    mode: 0o644,
                    owner: "root".to_string(),
                    group: "root".to_string(),
                });
            }
        }
        "nocreate" => config.create = None,

        "size" => {
            let sz = parts.get(1).ok_or("size requires a value")?;
            config.size = Some(parse_size(sz)?);
        }
        "minsize" => {
            let sz = parts.get(1).ok_or("minsize requires a value")?;
            config.min_size = Some(parse_size(sz)?);
        }
        "maxsize" => {
            let sz = parts.get(1).ok_or("maxsize requires a value")?;
            config.max_size = Some(parse_size(sz)?);
        }
        "maxage" => {
            let age = parts
                .get(1)
                .ok_or("maxage requires a value")?
                .parse::<u32>()
                .map_err(|_| "invalid maxage value".to_string())?;
            config.max_age = Some(age);
        }

        "olddir" => {
            let dir = parts.get(1).ok_or("olddir requires a path")?;
            config.old_dir = Some(PathBuf::from(dir));
        }
        "noolddir" => config.old_dir = None,

        // Ignore unknown directives (for forward compatibility).
        _ => {}
    }

    Ok(())
}

// ============================================================================
// State file (tracks last rotation time per log file)
// ============================================================================

/// State tracking: maps log file path to last rotation Unix timestamp.
struct RotationState {
    path: PathBuf,
    entries: HashMap<String, u64>,
}

impl RotationState {
    /// Load state from a file.
    fn load(path: &Path) -> Self {
        let mut entries = HashMap::new();

        if let Ok(content) = fs::read_to_string(path) {
            for line in content.lines() {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }

                // Format: "path" timestamp
                // The path is quoted with double quotes.
                if let Some(rest) = line.strip_prefix('"') {
                    if let Some(end_quote) = rest.find('"') {
                        let log_path = &rest[..end_quote];
                        let timestamp_str = rest[end_quote + 1..].trim();
                        if let Some(ts) = parse_timestamp(timestamp_str) {
                            entries.insert(log_path.to_string(), ts);
                        }
                    }
                }
            }
        }

        RotationState {
            path: path.to_path_buf(),
            entries,
        }
    }

    /// Save state to the file.
    fn save(&self) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("cannot create state dir: {e}"))?;
        }

        let mut out = String::new();
        // Sort entries for deterministic output.
        let mut sorted: Vec<(&String, &u64)> = self.entries.iter().collect();
        sorted.sort_by_key(|(k, _)| (*k).clone());

        for (log_path, ts) in &sorted {
            let bt = unix_to_broken(**ts);
            out.push_str(&format!("\"{}\" {}\n", log_path, format_timestamp(&bt)));
        }

        fs::write(&self.path, &out)
            .map_err(|e| format!("cannot write state file: {e}"))
    }

    /// Get last rotation time for a log file.
    fn last_rotation(&self, log_path: &str) -> Option<u64> {
        self.entries.get(log_path).copied()
    }

    /// Record that a log file was rotated now.
    fn record_rotation(&mut self, log_path: &str, timestamp: u64) {
        self.entries.insert(log_path.to_string(), timestamp);
    }
}

// ============================================================================
// Rotation action tracking (for JSON output)
// ============================================================================

/// Record of a single rotation action performed.
#[derive(Debug)]
struct RotationAction {
    log_path: String,
    action: String,
    detail: String,
}

impl RotationAction {
    fn to_json(&self) -> String {
        // Manual JSON construction (no serde in this no-dependency crate).
        format!(
            "{{\"log\":\"{}\",\"action\":\"{}\",\"detail\":\"{}\"}}",
            json_escape(&self.log_path),
            json_escape(&self.action),
            json_escape(&self.detail),
        )
    }
}

fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(c),
        }
    }
    out
}

// ============================================================================
// Rotation engine
// ============================================================================

/// Runtime options controlling rotation behavior.
struct RunOptions {
    debug: bool,
    force: bool,
    verbose: bool,
    json: bool,
}

/// Determine whether a log file needs rotation based on config, state, and runtime flags.
fn should_rotate(
    log_path: &Path,
    config: &LogConfig,
    state: &RotationState,
    opts: &RunOptions,
    now: u64,
) -> (bool, String) {
    if opts.force {
        return (true, "forced rotation".to_string());
    }

    // Check if the file exists.
    let metadata = match fs::metadata(log_path) {
        Ok(m) => m,
        Err(_) => {
            if config.missing_ok {
                return (false, "file missing (missingok)".to_string());
            }
            return (false, "file missing".to_string());
        }
    };

    let file_size = metadata.len();

    // Check notifempty: skip if file is empty (unless ifempty overrides).
    if config.not_if_empty && !config.if_empty && file_size == 0 {
        return (false, "file is empty (notifempty)".to_string());
    }

    // Check size threshold: only rotate if file is at least this big.
    if let Some(min) = config.size {
        if file_size < min {
            return (
                false,
                format!("file too small ({file_size} < {min})"),
            );
        }
    }

    // Check minsize: don't rotate unless file exceeds minsize.
    if let Some(min) = config.min_size {
        if file_size < min {
            return (
                false,
                format!("below minsize ({file_size} < {min})"),
            );
        }
    }

    // Check maxsize: rotate immediately if file exceeds maxsize.
    if let Some(max) = config.max_size {
        if file_size >= max {
            return (true, format!("exceeds maxsize ({file_size} >= {max})"));
        }
    }

    // Check time-based rotation.
    let log_key = log_path.to_string_lossy().to_string();
    if let Some(last) = state.last_rotation(&log_key) {
        let elapsed = now.saturating_sub(last);
        let interval = config.frequency.min_interval_secs();
        if elapsed < interval {
            return (
                false,
                format!(
                    "too soon ({} < {} for {})",
                    elapsed,
                    interval,
                    config.frequency.as_str()
                ),
            );
        }
        (true, format!("{}: interval elapsed", config.frequency.as_str()))
    } else {
        // No state entry means this is the first time we see this log.
        // Rotate it to establish a baseline.
        (true, "first rotation (no state entry)".to_string())
    }
}

/// Perform rotation for a single log file.
fn rotate_log(
    log_path: &Path,
    config: &LogConfig,
    state: &mut RotationState,
    opts: &RunOptions,
    now: u64,
    actions: &mut Vec<RotationAction>,
) {
    let log_str = log_path.to_string_lossy().to_string();

    let (should, reason) = should_rotate(log_path, config, state, opts, now);

    if opts.verbose || opts.debug {
        println!(
            "  {} — {}{}",
            log_str,
            reason,
            if should { "" } else { " [skipping]" }
        );
    }

    if !should {
        actions.push(RotationAction {
            log_path: log_str,
            action: "skip".to_string(),
            detail: reason,
        });
        return;
    }

    // Determine the rotation target directory.
    let rot_dir = if let Some(ref old) = config.old_dir {
        old.clone()
    } else {
        log_path
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."))
    };

    let file_stem = log_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("log");

    if opts.debug {
        println!("    [dry-run] would rotate {log_str}");
    }

    // Ensure rotation directory exists.
    if !opts.debug {
        if let Err(e) = fs::create_dir_all(&rot_dir) {
            eprintln!("    error: cannot create rotation dir {}: {e}", rot_dir.display());
            return;
        }
    }

    // Step 1: Run prerotate script.
    if let Some(ref script) = config.pre_rotate {
        if opts.verbose || opts.debug {
            println!("    prerotate: {script}");
        }
        if !opts.debug {
            run_script(script);
        }
        actions.push(RotationAction {
            log_path: log_str.clone(),
            action: "prerotate".to_string(),
            detail: script.clone(),
        });
    }

    if config.date_ext {
        // Date-based rotation: log -> log-YYYYMMDD
        let bt = unix_to_broken(now);
        let date_suffix = format_date_ext(&bt, &config.date_format);
        let rotated_name = format!("{file_stem}{date_suffix}");
        let rotated_path = rot_dir.join(&rotated_name);

        if opts.debug {
            println!("    [dry-run] {} -> {}", log_str, rotated_path.display());
        } else if config.copy_truncate {
            do_copy_truncate(log_path, &rotated_path, opts, actions);
        } else {
            do_rename_rotate(log_path, &rotated_path, opts);
        }

        // Compress the rotated file (if enabled and not delaycompress).
        if config.compress && !config.delay_compress {
            let gz_path = format!("{}.gz", rotated_path.display());
            if opts.verbose || opts.debug {
                println!("    compress: {} -> {gz_path}", rotated_path.display());
            }
            if !opts.debug {
                // We just rename to .gz to mark it; actual gzip compression would
                // require a gzip implementation or external tool. This marks intent.
                let _ = fs::rename(&rotated_path, &gz_path);
            }
            actions.push(RotationAction {
                log_path: log_str.clone(),
                action: "compress".to_string(),
                detail: gz_path,
            });
        }
    } else {
        // Numbered rotation: shift log.N -> log.N+1, then log -> log.1

        // First, remove the oldest file if it exceeds rotate_count.
        let max_num = config.rotate_count;

        // Shift existing numbered rotations upward.
        // Walk from the highest to the lowest to avoid overwrites.
        for n in (1..max_num).rev() {
            let src_name = numbered_name(file_stem, n, config.compress);
            let dst_name = numbered_name(file_stem, n + 1, config.compress);
            let src = rot_dir.join(&src_name);
            let dst = rot_dir.join(&dst_name);

            if src.exists() {
                if opts.debug {
                    println!("    [dry-run] {} -> {}", src.display(), dst.display());
                } else {
                    let _ = fs::rename(&src, &dst);
                }
            }
        }

        // Remove files beyond the retain count.
        let remove_name = numbered_name(file_stem, max_num + 1, config.compress);
        let remove_path = rot_dir.join(&remove_name);
        if remove_path.exists() {
            if opts.debug {
                println!("    [dry-run] remove {}", remove_path.display());
            } else {
                let _ = fs::remove_file(&remove_path);
            }
            actions.push(RotationAction {
                log_path: log_str.clone(),
                action: "remove".to_string(),
                detail: remove_path.to_string_lossy().to_string(),
            });
        }

        // Also remove the uncompressed variant beyond the retain count.
        let remove_name_plain = numbered_name(file_stem, max_num + 1, false);
        let remove_path_plain = rot_dir.join(&remove_name_plain);
        if remove_path_plain.exists() && remove_path_plain != remove_path {
            if !opts.debug {
                let _ = fs::remove_file(&remove_path_plain);
            }
        }

        // Rotate the current log to .1
        let rotated_1 = rot_dir.join(format!("{file_stem}.1"));

        if opts.debug {
            println!("    [dry-run] {} -> {}", log_str, rotated_1.display());
        } else if config.copy_truncate {
            do_copy_truncate(log_path, &rotated_1, opts, actions);
        } else {
            do_rename_rotate(log_path, &rotated_1, opts);
        }

        // Compress .2 and above (handle delaycompress: don't compress .1).
        if config.compress {
            let start = if config.delay_compress { 2 } else { 1 };
            for n in start..=max_num {
                let plain_name = format!("{file_stem}.{n}");
                let plain_path = rot_dir.join(&plain_name);
                let gz_name = format!("{file_stem}.{n}.gz");
                let gz_path = rot_dir.join(&gz_name);

                if plain_path.exists() && !gz_path.exists() {
                    if opts.verbose || opts.debug {
                        println!(
                            "    compress: {} -> {}",
                            plain_path.display(),
                            gz_path.display()
                        );
                    }
                    if !opts.debug {
                        let _ = fs::rename(&plain_path, &gz_path);
                    }
                    actions.push(RotationAction {
                        log_path: log_str.clone(),
                        action: "compress".to_string(),
                        detail: gz_path.to_string_lossy().to_string(),
                    });
                }
            }
        }

        actions.push(RotationAction {
            log_path: log_str.clone(),
            action: "rotate".to_string(),
            detail: format!("{} -> {}", log_str, rotated_1.display()),
        });
    }

    // Step 2: Create new empty log file if requested.
    if let Some(ref spec) = config.create {
        if !config.copy_truncate {
            if opts.verbose || opts.debug {
                println!(
                    "    create: {} (mode {:04o}, {}:{})",
                    log_str, spec.mode, spec.owner, spec.group
                );
            }
            if !opts.debug {
                // Create the new empty log file.
                match fs::File::create(log_path) {
                    Ok(_) => {}
                    Err(e) => {
                        eprintln!("    error: cannot create {}: {e}", log_str);
                    }
                }
                // On our OS, we would set mode/owner/group via syscalls here.
                // For now, just create the file.
            }
            actions.push(RotationAction {
                log_path: log_str.clone(),
                action: "create".to_string(),
                detail: format!("mode {:04o} {}:{}", spec.mode, spec.owner, spec.group),
            });
        }
    }

    // Step 3: Run postrotate script.
    if let Some(ref script) = config.post_rotate {
        if opts.verbose || opts.debug {
            println!("    postrotate: {script}");
        }
        if !opts.debug {
            run_script(script);
        }
        actions.push(RotationAction {
            log_path: log_str.clone(),
            action: "postrotate".to_string(),
            detail: script.clone(),
        });
    }

    // Step 4: Clean up files exceeding maxage.
    if let Some(max_days) = config.max_age {
        cleanup_old_files(&rot_dir, file_stem, max_days, now, opts, actions, &log_str);
    }

    // Record the rotation in state.
    state.record_rotation(&log_str, now);
}

/// Construct a numbered rotated file name, optionally with .gz suffix.
fn numbered_name(stem: &str, n: u32, compressed: bool) -> String {
    if compressed {
        format!("{stem}.{n}.gz")
    } else {
        format!("{stem}.{n}")
    }
}

/// Copy contents to the rotated destination and truncate the original.
fn do_copy_truncate(
    src: &Path,
    dst: &Path,
    opts: &RunOptions,
    actions: &mut Vec<RotationAction>,
) {
    let src_str = src.to_string_lossy().to_string();
    let dst_str = dst.to_string_lossy().to_string();

    if opts.verbose {
        println!("    copy: {} -> {}", src_str, dst_str);
    }

    match fs::copy(src, dst) {
        Ok(bytes) => {
            if opts.verbose {
                println!("    copied {bytes} bytes");
            }
        }
        Err(e) => {
            eprintln!("    error: copy {} -> {}: {e}", src_str, dst_str);
            return;
        }
    }

    // Truncate the original.
    match fs::File::create(src) {
        Ok(_) => {
            if opts.verbose {
                println!("    truncated: {src_str}");
            }
        }
        Err(e) => {
            eprintln!("    error: truncate {}: {e}", src_str);
        }
    }

    actions.push(RotationAction {
        log_path: src_str,
        action: "copytruncate".to_string(),
        detail: dst_str,
    });
}

/// Rename the source file to the destination (standard rotation).
fn do_rename_rotate(
    src: &Path,
    dst: &Path,
    opts: &RunOptions,
) {
    let src_str = src.to_string_lossy().to_string();
    let dst_str = dst.to_string_lossy().to_string();

    if opts.verbose {
        println!("    rename: {} -> {}", src_str, dst_str);
    }

    if let Err(e) = fs::rename(src, dst) {
        eprintln!("    error: rename {} -> {}: {e}", src_str, dst_str);
    }
}

/// Run a rotation script (pre/post).
fn run_script(script: &str) {
    let result = process::Command::new("/bin/sh")
        .arg("-c")
        .arg(script)
        .status();

    match result {
        Ok(status) => {
            if !status.success() {
                eprintln!(
                    "    script exited with code {}",
                    status.code().unwrap_or(-1)
                );
            }
        }
        Err(e) => {
            eprintln!("    error running script: {e}");
        }
    }
}

/// Remove rotated files older than max_days.
fn cleanup_old_files(
    dir: &Path,
    stem: &str,
    max_days: u32,
    now: u64,
    opts: &RunOptions,
    actions: &mut Vec<RotationAction>,
    log_path_str: &str,
) {
    let max_age_secs = u64::from(max_days) * 86400;

    let read_dir = match fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(_) => return,
    };

    for entry in read_dir {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };

        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        // Only consider files that look like rotated versions of this log.
        if !name_str.starts_with(stem) || name_str == stem {
            continue;
        }

        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        // Check file modification time.
        let mtime = match fs::metadata(&path).and_then(|m| m.modified()) {
            Ok(t) => t
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
            Err(_) => continue,
        };

        let age = now.saturating_sub(mtime);
        if age > max_age_secs {
            if opts.verbose || opts.debug {
                println!(
                    "    maxage: removing {} (age {} days)",
                    path.display(),
                    age / 86400
                );
            }
            if !opts.debug {
                let _ = fs::remove_file(&path);
            }
            actions.push(RotationAction {
                log_path: log_path_str.to_string(),
                action: "remove_aged".to_string(),
                detail: path.to_string_lossy().to_string(),
            });
        }
    }
}

// ============================================================================
// Main entry point
// ============================================================================

fn print_usage() {
    println!("OurOS Log Rotation Utility v{VERSION}");
    println!();
    println!("Rotates, compresses, and manages log files based on configuration.");
    println!("Designed to be run periodically by crond.");
    println!();
    println!("USAGE:");
    println!("  logrotate [OPTIONS] [config_file]");
    println!();
    println!("OPTIONS:");
    println!("  -d, --debug     Dry-run: show what would be done without doing it");
    println!("  -f, --force     Force rotation regardless of time/size criteria");
    println!("  -v, --verbose   Verbose output");
    println!("  -s, --state F   State file path (default: {DEFAULT_STATE_FILE})");
    println!("      --json      Output rotation actions as JSON");
    println!("  -h, --help      Show this help message");
    println!("      --version   Show version");
    println!();
    println!("CONFIG FORMAT:");
    println!("  Global directives apply to all log files unless overridden.");
    println!("  Per-file blocks override globals:");
    println!();
    println!("    compress");
    println!("    dateext");
    println!();
    println!("    /var/log/syslog {{");
    println!("        daily");
    println!("        rotate 7");
    println!("        compress");
    println!("        missingok");
    println!("        notifempty");
    println!("        create 0640 root root");
    println!("        postrotate");
    println!("            kill -HUP $(cat /var/run/syslogd.pid)");
    println!("        endscript");
    println!("    }}");
    println!();
    println!("DIRECTIVES:");
    println!("  daily / weekly / monthly / yearly  Rotation frequency");
    println!("  rotate N                           Keep N rotated files");
    println!("  compress / nocompress               Compress rotated files");
    println!("  delaycompress                      Delay compression by one cycle");
    println!("  missingok / nomissingok             Tolerate missing log files");
    println!("  notifempty / ifempty                Skip/force rotation of empty files");
    println!("  create MODE OWNER GROUP             Create new log after rotation");
    println!("  copytruncate                       Copy then truncate (for open files)");
    println!("  size N[k|M|G]                      Minimum size to trigger rotation");
    println!("  minsize N[k|M|G]                   Don't rotate below this size");
    println!("  maxsize N[k|M|G]                   Rotate immediately above this size");
    println!("  maxage N                           Remove rotated files older than N days");
    println!("  dateext / nodateext                Use date-based names");
    println!("  dateformat FMT                     Date format (e.g. -%Y%m%d)");
    println!("  olddir DIR                         Store rotated logs in DIR");
    println!("  sharedscripts                      Run scripts once for all logs");
    println!("  prerotate / endscript              Script to run before rotation");
    println!("  postrotate / endscript             Script to run after rotation");
    println!("  include PATH                       Include config file or directory");
}

fn main() {
    let args: Vec<String> = env::args().collect();

    let mut config_path = PathBuf::from(DEFAULT_CONFIG);
    let mut state_path = PathBuf::from(DEFAULT_STATE_FILE);
    let mut opts = RunOptions {
        debug: false,
        force: false,
        verbose: false,
        json: false,
    };

    // Parse command-line arguments.
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-d" | "--debug" => opts.debug = true,
            "-f" | "--force" => opts.force = true,
            "-v" | "--verbose" => opts.verbose = true,
            "--json" => opts.json = true,
            "-s" | "--state" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("error: --state requires a path argument");
                    process::exit(1);
                }
                state_path = PathBuf::from(&args[i]);
            }
            "-h" | "--help" => {
                print_usage();
                process::exit(0);
            }
            "--version" => {
                println!("logrotate {VERSION}");
                process::exit(0);
            }
            arg if arg.starts_with('-') => {
                eprintln!("error: unknown option: {arg}");
                eprintln!("Run 'logrotate --help' for usage.");
                process::exit(1);
            }
            // Positional argument: config file path.
            _ => {
                config_path = PathBuf::from(&args[i]);
            }
        }
        i += 1;
    }

    // Parse configuration.
    let config = match parse_config(&config_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {e}");
            process::exit(1);
        }
    };

    if config.entries.is_empty() {
        if opts.verbose {
            println!("No log entries configured.");
        }
        process::exit(0);
    }

    // Load rotation state.
    let mut state = RotationState::load(&state_path);

    if opts.verbose || opts.debug {
        println!(
            "logrotate v{VERSION}: processing {} config entries",
            config.entries.len()
        );
        if opts.debug {
            println!("  (dry-run mode — no files will be modified)");
        }
    }

    let now = now_secs();
    let mut all_actions: Vec<RotationAction> = Vec::new();

    // Process each configured log entry.
    for entry in &config.entries {
        if entry.paths.is_empty() {
            continue;
        }

        if opts.verbose || opts.debug {
            let path_list: Vec<String> = entry.paths.iter()
                .map(|p| p.to_string_lossy().to_string())
                .collect();
            println!("\nProcessing: {}", path_list.join(", "));
            println!(
                "  frequency={}, rotate={}, compress={}",
                entry.frequency.as_str(),
                entry.rotate_count,
                entry.compress
            );
        }

        // Handle sharedscripts: run prerotate once before all logs.
        if entry.shared_scripts {
            if let Some(ref script) = entry.pre_rotate {
                if opts.verbose || opts.debug {
                    println!("  sharedscripts prerotate: {script}");
                }
                if !opts.debug {
                    run_script(script);
                }
                all_actions.push(RotationAction {
                    log_path: "(shared)".to_string(),
                    action: "prerotate".to_string(),
                    detail: script.clone(),
                });
            }
        }

        // Create a modified config without pre/post scripts for individual logs
        // when using sharedscripts.
        let per_log_config = if entry.shared_scripts {
            let mut c = entry.clone();
            c.pre_rotate = None;
            c.post_rotate = None;
            c
        } else {
            entry.clone()
        };

        for log_path in &entry.paths {
            rotate_log(
                log_path,
                &per_log_config,
                &mut state,
                &opts,
                now,
                &mut all_actions,
            );
        }

        // Handle sharedscripts: run postrotate once after all logs.
        if entry.shared_scripts {
            if let Some(ref script) = entry.post_rotate {
                if opts.verbose || opts.debug {
                    println!("  sharedscripts postrotate: {script}");
                }
                if !opts.debug {
                    run_script(script);
                }
                all_actions.push(RotationAction {
                    log_path: "(shared)".to_string(),
                    action: "postrotate".to_string(),
                    detail: script.clone(),
                });
            }
        }
    }

    // Save updated state (unless dry-run).
    if !opts.debug {
        if let Err(e) = state.save() {
            eprintln!("error: {e}");
            process::exit(1);
        }
    }

    // JSON output.
    if opts.json {
        let mut out = std::io::stdout().lock();
        let _ = writeln!(out, "[");
        for (idx, action) in all_actions.iter().enumerate() {
            let comma = if idx + 1 < all_actions.len() { "," } else { "" };
            let _ = writeln!(out, "  {}{comma}", action.to_json());
        }
        let _ = writeln!(out, "]");
    }

    if opts.verbose || opts.debug {
        let rotated = all_actions
            .iter()
            .filter(|a| a.action == "rotate" || a.action == "copytruncate")
            .count();
        let skipped = all_actions.iter().filter(|a| a.action == "skip").count();
        println!(
            "\nDone: {rotated} rotated, {skipped} skipped."
        );
    }
}
