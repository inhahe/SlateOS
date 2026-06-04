//! OurOS Kernel Message Buffer Viewer
//!
//! Reads and displays kernel log messages from /proc/kmsg or /var/log/kernel.
//! Similar to Linux `dmesg` — essential for boot diagnostics and driver debugging.
//!
//! # Usage
//!
//! ```text
//! dmesg                    Show all kernel messages
//! dmesg -n <count>         Show last N messages
//! dmesg -f                 Follow (live tail, like tail -f)
//! dmesg -l <level>         Filter by level (emerg/alert/crit/err/warn/notice/info/debug)
//! dmesg -s <string>        Search messages for substring
//! dmesg -c                 Clear the ring buffer after reading
//! dmesg -T                 Show human-readable timestamps
//! dmesg --json             Output as JSON-lines
//! dmesg --since <secs>     Show messages from last N seconds
//! dmesg --boot             Show only messages from current boot
//! ```

use std::env;
use std::fs;
use std::process;
use std::thread;
use std::time::Duration;

// ============================================================================
// Log levels
// ============================================================================

#[derive(Clone, Copy, PartialEq, PartialOrd)]
#[repr(u8)]
enum LogLevel {
    Emergency = 0,
    Alert = 1,
    Critical = 2,
    Error = 3,
    Warning = 4,
    Notice = 5,
    Info = 6,
    Debug = 7,
}

impl LogLevel {
    fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "emerg" | "emergency" | "0" => Some(Self::Emergency),
            "alert" | "1" => Some(Self::Alert),
            "crit" | "critical" | "2" => Some(Self::Critical),
            "err" | "error" | "3" => Some(Self::Error),
            "warn" | "warning" | "4" => Some(Self::Warning),
            "notice" | "5" => Some(Self::Notice),
            "info" | "6" => Some(Self::Info),
            "debug" | "7" => Some(Self::Debug),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Emergency => "emerg",
            Self::Alert => "alert",
            Self::Critical => "crit",
            Self::Error => "err",
            Self::Warning => "warn",
            Self::Notice => "notice",
            Self::Info => "info",
            Self::Debug => "debug",
        }
    }

    fn ansi_color(self) -> &'static str {
        match self {
            Self::Emergency | Self::Alert => "\x1b[1;31m", // bold red
            Self::Critical | Self::Error => "\x1b[31m",     // red
            Self::Warning => "\x1b[33m",                     // yellow
            Self::Notice => "\x1b[36m",                      // cyan
            Self::Info => "",                                 // default
            Self::Debug => "\x1b[90m",                       // dim gray
        }
    }
}

// ============================================================================
// Kernel message parsing
// ============================================================================

struct KernelMessage {
    /// Timestamp in microseconds since boot.
    timestamp_us: u64,
    /// Log level.
    level: LogLevel,
    /// Subsystem/facility name.
    facility: String,
    /// Message text.
    message: String,
}

/// Parse a kernel message line.
///
/// Our kernel uses the format:
///   `<level>,sequence,timestamp_us,facility;message`
///
/// Or from /var/log/kernel (JSON-lines):
///   `{"ts":N,"level":"info","service":"kernel","msg":"..."}`
///
/// Falls back to treating the entire line as a plain message at Info level.
fn parse_kmsg_line(line: &str) -> Option<KernelMessage> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }

    // Try JSON-lines format first (from syslogd).
    if line.starts_with('{') {
        return parse_json_kmsg(line);
    }

    // Try structured kmsg format: <level>,seq,timestamp,facility;message
    if let Some((prefix, message)) = line.split_once(';') {
        let parts: Vec<&str> = prefix.split(',').collect();
        if parts.len() >= 3 {
            let level_num: u8 = parts[0].parse().unwrap_or(6);
            let level = match level_num {
                0 => LogLevel::Emergency,
                1 => LogLevel::Alert,
                2 => LogLevel::Critical,
                3 => LogLevel::Error,
                4 => LogLevel::Warning,
                5 => LogLevel::Notice,
                6 => LogLevel::Info,
                _ => LogLevel::Debug,
            };
            let timestamp_us: u64 = parts[2].parse().unwrap_or(0);
            let facility = if parts.len() >= 4 {
                parts[3].trim_matches('-').to_string()
            } else {
                String::new()
            };

            return Some(KernelMessage {
                timestamp_us,
                level,
                facility,
                message: message.to_string(),
            });
        }
    }

    // Plain text fallback.
    Some(KernelMessage {
        timestamp_us: 0,
        level: LogLevel::Info,
        facility: String::new(),
        message: line.to_string(),
    })
}

/// Parse a JSON-lines kernel message.
fn parse_json_kmsg(line: &str) -> Option<KernelMessage> {
    // Minimal JSON extraction — no full parser needed.
    let ts = extract_json_number(line, "ts").unwrap_or(0);
    let level_str = extract_json_string(line, "level").unwrap_or_default();
    let msg = extract_json_string(line, "msg").unwrap_or_default();
    let service = extract_json_string(line, "service").unwrap_or_default();

    let level = LogLevel::from_str(&level_str).unwrap_or(LogLevel::Info);

    Some(KernelMessage {
        timestamp_us: ts * 1_000_000, // ts is in seconds
        level,
        facility: service,
        message: msg,
    })
}

fn extract_json_string(json: &str, key: &str) -> Option<String> {
    let search = format!("\"{}\":\"", key);
    let start = json.find(&search)? + search.len();
    let rest = &json[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn extract_json_number(json: &str, key: &str) -> Option<u64> {
    let search = format!("\"{}\":", key);
    let start = json.find(&search)? + search.len();
    let rest = json[start..].trim_start();
    let end = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len());
    rest[..end].parse().ok()
}

// ============================================================================
// Message sources
// ============================================================================

/// Read kernel messages from available sources.
fn read_kernel_messages() -> Vec<KernelMessage> {
    // Try /proc/kmsg first (live kernel ring buffer).
    if let Some(content) = read_file("/proc/kmsg") {
        return content.lines()
            .filter_map(parse_kmsg_line)
            .collect();
    }

    // Fall back to /var/log/kernel (syslogd-managed).
    if let Some(content) = read_file("/var/log/kernel") {
        return content.lines()
            .filter_map(parse_kmsg_line)
            .collect();
    }

    // Try /var/log/syslog as last resort.
    if let Some(content) = read_file("/var/log/syslog") {
        return content.lines()
            .filter_map(parse_kmsg_line)
            .collect();
    }

    Vec::new()
}

fn read_file(path: &str) -> Option<String> {
    fs::read_to_string(path).ok()
}

// ============================================================================
// Display
// ============================================================================

struct Config {
    count: Option<usize>,
    follow: bool,
    level_filter: Option<LogLevel>,
    search: Option<String>,
    clear: bool,
    human_time: bool,
    json_output: bool,
    since_secs: Option<u64>,
    color: bool,
}

fn format_timestamp_us(us: u64, human: bool) -> String {
    if human {
        let secs = us / 1_000_000;
        let hours = secs / 3600;
        let mins = (secs % 3600) / 60;
        let s = secs % 60;
        let ms = (us % 1_000_000) / 1000;
        format!("[{hours:02}:{mins:02}:{s:02}.{ms:03}]")
    } else {
        let secs = us as f64 / 1_000_000.0;
        format!("[{secs:>12.6}]")
    }
}

fn display_message(msg: &KernelMessage, config: &Config) {
    if config.json_output {
        println!(
            "{{\"timestamp_us\":{},\"level\":\"{}\",\"facility\":\"{}\",\"message\":\"{}\"}}",
            msg.timestamp_us,
            msg.level.name(),
            json_escape(&msg.facility),
            json_escape(&msg.message),
        );
        return;
    }

    let ts = format_timestamp_us(msg.timestamp_us, config.human_time);

    let level_tag = format!("{:<6}", msg.level.name());

    let facility_str = if msg.facility.is_empty() {
        String::new()
    } else {
        format!("{}: ", msg.facility)
    };

    if config.color {
        let color = msg.level.ansi_color();
        let reset = if color.is_empty() { "" } else { "\x1b[0m" };
        println!("{ts} {color}{level_tag}{reset} {facility_str}{}", msg.message);
    } else {
        println!("{ts} {level_tag} {facility_str}{}", msg.message);
    }
}

fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out
}

/// Get current uptime in microseconds for --since filtering.
fn current_uptime_us() -> u64 {
    read_file("/proc/uptime")
        .and_then(|s| {
            s.split_whitespace()
                .next()
                .and_then(|v| v.parse::<f64>().ok())
        })
        .map(|secs| (secs * 1_000_000.0) as u64)
        .unwrap_or(0)
}

// ============================================================================
// Main
// ============================================================================

fn print_usage() {
    println!("OurOS Kernel Message Viewer v0.1.0");
    println!();
    println!("Display kernel ring buffer messages for boot diagnostics and debugging.");
    println!();
    println!("USAGE:");
    println!("  dmesg [options]");
    println!();
    println!("OPTIONS:");
    println!("  -n <count>      Show last N messages");
    println!("  -f, --follow    Follow (live tail)");
    println!("  -l <level>      Filter by minimum level:");
    println!("                    emerg, alert, crit, err, warn, notice, info, debug");
    println!("  -s <string>     Search for substring in messages");
    println!("  -c              Clear the ring buffer after reading");
    println!("  -T              Human-readable timestamps (HH:MM:SS.mmm)");
    println!("  --json          JSON-lines output");
    println!("  --since <secs>  Messages from last N seconds only");
    println!("  --nocolor       Disable colored output");
    println!("  --help, -h      Show this help");
    println!();
    println!("LEVELS (lowest to highest priority):");
    println!("  debug < info < notice < warn < err < crit < alert < emerg");
}

fn main() {
    let args: Vec<String> = env::args().collect();

    let mut config = Config {
        count: None,
        follow: false,
        level_filter: None,
        search: None,
        clear: false,
        human_time: false,
        json_output: false,
        since_secs: None,
        color: true,
    };

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-n" => {
                if i + 1 >= args.len() {
                    eprintln!("error: -n requires a count");
                    process::exit(1);
                }
                config.count = Some(args[i + 1].parse().unwrap_or(20));
                i += 2;
            }
            "-f" | "--follow" => {
                config.follow = true;
                i += 1;
            }
            "-l" | "--level" => {
                if i + 1 >= args.len() {
                    eprintln!("error: -l requires a level name");
                    process::exit(1);
                }
                config.level_filter = match LogLevel::from_str(&args[i + 1]) {
                    Some(l) => Some(l),
                    None => {
                        eprintln!("error: unknown level: {}", args[i + 1]);
                        process::exit(1);
                    }
                };
                i += 2;
            }
            "-s" | "--search" => {
                if i + 1 >= args.len() {
                    eprintln!("error: -s requires a search string");
                    process::exit(1);
                }
                config.search = Some(args[i + 1].clone());
                i += 2;
            }
            "-c" | "--clear" => {
                config.clear = true;
                i += 1;
            }
            "-T" | "--human-time" => {
                config.human_time = true;
                i += 1;
            }
            "--json" => {
                config.json_output = true;
                config.color = false;
                i += 1;
            }
            "--since" => {
                if i + 1 >= args.len() {
                    eprintln!("error: --since requires seconds");
                    process::exit(1);
                }
                config.since_secs = Some(args[i + 1].parse().unwrap_or(60));
                i += 2;
            }
            "--nocolor" => {
                config.color = false;
                i += 1;
            }
            "--help" | "-h" | "help" => {
                print_usage();
                process::exit(0);
            }
            other => {
                eprintln!("unknown option: {other}");
                eprintln!("Run 'dmesg --help' for usage.");
                process::exit(1);
            }
        }
    }

    // Read messages.
    let mut messages = read_kernel_messages();

    if messages.is_empty() {
        eprintln!("(no kernel messages available)");
        if !config.follow {
            process::exit(0);
        }
    }

    // Filter by --since.
    if let Some(since) = config.since_secs {
        let uptime = current_uptime_us();
        let cutoff = uptime.saturating_sub(since * 1_000_000);
        messages.retain(|m| m.timestamp_us >= cutoff);
    }

    // Filter by level.
    if let Some(min_level) = config.level_filter {
        // Keep messages at or above the specified severity.
        // Lower numeric value = higher severity.
        messages.retain(|m| (m.level as u8) <= (min_level as u8));
    }

    // Filter by search.
    if let Some(ref search) = config.search {
        let search_lower = search.to_lowercase();
        messages.retain(|m| {
            m.message.to_lowercase().contains(&search_lower)
                || m.facility.to_lowercase().contains(&search_lower)
        });
    }

    // Limit to last N.
    if let Some(count) = config.count
        && messages.len() > count {
            messages = messages.split_off(messages.len() - count);
        }

    // Display.
    for msg in &messages {
        display_message(msg, &config);
    }

    // Clear ring buffer if requested.
    if config.clear {
        // Attempt to truncate /proc/kmsg (kernel honors this as a clear).
        if fs::write("/proc/kmsg", "").is_ok() {
            eprintln!("(ring buffer cleared)");
        } else {
            eprintln!("(could not clear ring buffer — permission denied?)");
        }
    }

    // Follow mode: poll for new messages.
    if config.follow {
        let mut last_count = messages.len();
        loop {
            thread::sleep(Duration::from_secs(1));

            let new_messages = read_kernel_messages();
            if new_messages.len() > last_count {
                for msg in new_messages.iter().skip(last_count) {
                    // Apply filters.
                    if let Some(min_level) = config.level_filter
                        && (msg.level as u8) > (min_level as u8) {
                            continue;
                        }
                    if let Some(ref search) = config.search {
                        let search_lower = search.to_lowercase();
                        if !msg.message.to_lowercase().contains(&search_lower)
                            && !msg.facility.to_lowercase().contains(&search_lower)
                        {
                            continue;
                        }
                    }
                    display_message(msg, &config);
                }
                last_count = new_messages.len();
            }
        }
    }
}
