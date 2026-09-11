//! Slate OS Kernel Message Buffer Viewer
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

#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
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
    /// The line carried no level, or one this program does not recognise.
    ///
    /// **Not a severity**, which is why it sits outside the 0..=7 range the
    /// kernel uses and why the level filter treats it specially rather than
    /// comparing it. It used to be `Info`: `parse_json_kmsg` ended with
    /// `LogLevel::from_str(&level_str).unwrap_or(LogLevel::Info)`, and `-l`
    /// keeps messages whose numeric level is at or below the requested one --
    /// so a message with a malformed level was silently reclassified as Info
    /// (6) and then dropped from `dmesg -l err` (3).
    ///
    /// That is the wrong direction for a log viewer. Filtering narrows, and
    /// narrowing away a line you could not classify hides the evidence
    /// somebody is filtering in order to find.
    Unknown = 255,
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
            Self::Unknown => "unknown",
        }
    }

    fn ansi_color(self) -> &'static str {
        match self {
            Self::Emergency | Self::Alert => "\x1b[1;31m", // bold red
            Self::Critical | Self::Error => "\x1b[31m",    // red
            Self::Warning => "\x1b[33m",                   // yellow
            Self::Notice => "\x1b[36m",                    // cyan
            Self::Info => "",                              // default
            Self::Debug => "\x1b[90m",                     // dim gray
            Self::Unknown => "\x1b[35m",                   // magenta: not a severity
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
    // A RECORD WITH NO `msg` FIELD IS NOT A MESSAGE. This was
    // `.unwrap_or_default()`, so a line that began with `{` but carried no
    // message -- a truncated write, a different JSON schema, a stray brace --
    // became a message whose text was the empty string, and dmesg printed a
    // blank line for it at timestamp 0.
    //
    // `?` and not a check for emptiness: `{"msg":""}` is a record that says
    // the message is empty, which is a different thing from one that does not
    // say anything, and `Some("")` keeps it.
    let msg = extract_json_string(line, "msg")?;
    let service = extract_json_string(line, "service").unwrap_or_default();

    // NOT `unwrap_or(LogLevel::Info)`. A level this program cannot read is not
    // an informational message; see `LogLevel::Unknown`.
    let level = LogLevel::from_str(&level_str).unwrap_or(LogLevel::Unknown);

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
    let end = rest
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(rest.len());
    rest[..end].parse().ok()
}

// ============================================================================
// Message sources
// ============================================================================

/// Read kernel messages from available sources.
/// The kernel's own log ring, through the only interface that reaches it.
///
/// `klogctl(SYSLOG_ACTION_READ_ALL)` is what `dmesg` uses on Linux, and what
/// `posix` wires to `SYS_LOG_READ` (102). It does not consume the ring, so
/// running `dmesg` twice shows the same messages twice rather than nothing the
/// second time.
///
/// This replaces reading `/proc/kmsg`, which **this kernel does not serve**:
/// there is no `kmsg` entry anywhere in `kernel/src/fs/procfs.rs`, so the open
/// failed, `dmesg` fell through to `/var/log/kernel` and `/var/log/syslog`, and
/// the tool for reading kernel messages never read the kernel's messages. The
/// file fallbacks below are kept because they are legitimate on a
/// syslogd-managed system -- they were just never the primary source.
fn read_kernel_ring() -> Option<String> {
    // The ring's own capacity, so a large log is not truncated and a small one
    // does not cost a megabyte. If the kernel will not say, take Linux's usual
    // default rather than guessing small.
    let size = libcall::klog_size().unwrap_or(1 << 17).clamp(4096, 1 << 22);
    let mut buf = vec![0u8; size];
    let n = libcall::klog_read_all(&mut buf).ok()?;
    buf.truncate(n);
    // Deliberately not lossy. Kernel log text is ASCII in practice, and if it
    // ever is not, the right answer is to fall through to the file sources
    // rather than to hand the parser replacement characters that were never in
    // the log. `read_file` below has always had exactly this property, via
    // `read_to_string`.
    String::from_utf8(buf).ok()
}

fn read_kernel_messages() -> Vec<KernelMessage> {
    // The live kernel ring, which is the point of the tool.
    if let Some(content) = read_kernel_ring() {
        return content.lines().filter_map(parse_kmsg_line).collect();
    }

    // Fall back to /var/log/kernel (syslogd-managed).
    if let Some(content) = read_file("/var/log/kernel") {
        return content.lines().filter_map(parse_kmsg_line).collect();
    }

    // Try /var/log/syslog as last resort.
    if let Some(content) = read_file("/var/log/syslog") {
        return content.lines().filter_map(parse_kmsg_line).collect();
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
        println!(
            "{ts} {color}{level_tag}{reset} {facility_str}{}",
            msg.message
        );
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
    println!("Slate OS Kernel Message Viewer v0.1.0");
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
        // `Unknown` is kept by every filter. It cannot be shown to be below
        // the threshold, and dropping it would hide exactly the line whose
        // severity nobody could establish.
        messages.retain(|m| m.level == LogLevel::Unknown || (m.level as u8) <= (min_level as u8));
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
        && messages.len() > count
    {
        messages = messages.split_off(messages.len() - count);
    }

    // Display.
    for msg in &messages {
        display_message(msg, &config);
    }

    // Clear ring buffer if requested.
    if config.clear {
        // `klogctl(SYSLOG_ACTION_CLEAR)`, which is how Linux clears the ring.
        //
        // This wrote an empty string to /proc/kmsg under a comment saying "the
        // kernel honors this as a clear". It does not, and neither does Linux;
        // and procfs here serves no `kmsg` entry at all, so the write could
        // only ever fail. What it printed then was the giveaway: "(could not
        // clear ring buffer — permission denied?)", with a question mark,
        // guessing at a reason for a failure it had not asked about. EPERM is
        // now reported because the kernel said EPERM, which for this action it
        // genuinely does without CAP_SYSLOG.
        match libcall::klog_clear() {
            Ok(()) => eprintln!("(ring buffer cleared)"),
            Err(libcall::EPERM) => {
                eprintln!("dmesg: cannot clear the ring buffer: not permitted (needs CAP_SYSLOG)");
            }
            Err(libcall::ENOSYS) => {
                eprintln!("dmesg: cannot clear the ring buffer: not implemented on this kernel");
            }
            Err(e) => eprintln!("dmesg: cannot clear the ring buffer: errno {e}"),
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
                        && msg.level != LogLevel::Unknown
                        && (msg.level as u8) > (min_level as u8)
                    {
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

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// The structured `kmsg` format: `<level>,seq,timestamp,facility;message`.
    #[test]
    fn a_structured_kmsg_line_parses_into_its_four_parts() {
        let m = parse_kmsg_line("3,42,1234567,kernel;disk failure").unwrap();
        assert_eq!(m.level, LogLevel::Error);
        assert_eq!(m.timestamp_us, 1_234_567);
        assert_eq!(m.facility, "kernel");
        assert_eq!(m.message, "disk failure");
    }

    /// Every Linux log level maps to its own name, and nothing above 7 is
    /// silently promoted into a more serious one.
    #[test]
    fn every_syslog_level_maps_to_its_own_severity() {
        let cases = [
            (0, LogLevel::Emergency),
            (1, LogLevel::Alert),
            (2, LogLevel::Critical),
            (3, LogLevel::Error),
            (4, LogLevel::Warning),
            (5, LogLevel::Notice),
            (6, LogLevel::Info),
            (7, LogLevel::Debug),
        ];
        for (n, want) in cases {
            let line = format!("{n},1,0,kernel;x");
            assert_eq!(parse_kmsg_line(&line).unwrap().level, want, "level {n}");
        }
    }

    /// A message with a semicolon in it keeps all of it.
    ///
    /// `split_once` rather than `split`, which matters because kernel messages
    /// contain semicolons -- a device path, an option list -- and splitting on
    /// every one would truncate the message at the first.
    #[test]
    fn a_semicolon_inside_the_message_is_not_a_separator() {
        let m = parse_kmsg_line("6,1,0,kernel;opts: a=1; b=2; c=3").unwrap();
        assert_eq!(m.message, "opts: a=1; b=2; c=3");
    }

    /// Anything that is not the structured format is still shown, not dropped.
    #[test]
    fn plain_text_falls_back_rather_than_vanishing() {
        let m = parse_kmsg_line("just some text").unwrap();
        assert_eq!(m.message, "just some text");
        assert_eq!(m.level, LogLevel::Info);
        assert_eq!(m.timestamp_us, 0);
    }

    /// Blank lines are not messages.
    #[test]
    fn an_empty_line_is_not_a_message() {
        assert!(parse_kmsg_line("").is_none());
        assert!(parse_kmsg_line("   ").is_none());
    }

    /// A prefix with too few fields is treated as plain text rather than
    /// half-parsed.
    #[test]
    fn a_short_prefix_is_plain_text_not_a_partial_parse() {
        let m = parse_kmsg_line("6,1;hello").unwrap();
        assert_eq!(m.message, "6,1;hello");
        assert_eq!(m.timestamp_us, 0);
    }

    #[test]
    fn json_escaping_covers_the_characters_that_would_break_a_parser() {
        assert_eq!(json_escape("a\"b"), "a\\\"b");
        assert_eq!(json_escape("a\\b"), "a\\\\b");
        assert_eq!(json_escape("a\nb"), "a\\nb");
        assert_eq!(json_escape("plain"), "plain");
    }

    /// A level nobody could read must not be filtered away.
    ///
    /// It used to become `Info` (6), and `-l err` keeps `level <= 3`, so a
    /// message whose level field was malformed vanished from the view somebody
    /// opened specifically to find problems.
    #[test]
    fn an_unreadable_level_survives_a_level_filter() {
        let line = r#"{"ts":1,"level":"not-a-level","msg":"disk on fire"}"#;
        let m = parse_kmsg_line(line).expect("a line with a message is a message");
        assert_eq!(
            m.level,
            LogLevel::Unknown,
            "an unrecognised level is Unknown"
        );

        // The filter's own rule, applied directly: Unknown is kept whatever
        // the threshold, and a real Info is not kept at `err`.
        let keeps =
            |lvl: LogLevel, min: LogLevel| lvl == LogLevel::Unknown || (lvl as u8) <= (min as u8);
        assert!(
            keeps(LogLevel::Unknown, LogLevel::Error),
            "must survive -l err"
        );
        assert!(
            keeps(LogLevel::Unknown, LogLevel::Emergency),
            "and -l emerg"
        );
        assert!(
            !keeps(LogLevel::Info, LogLevel::Error),
            "real Info is filtered"
        );
        assert!(
            keeps(LogLevel::Critical, LogLevel::Error),
            "crit passes err"
        );
    }

    /// `Unknown` is not a severity and must not sort as one.
    #[test]
    fn unknown_is_outside_the_kernel_severity_range() {
        assert!((LogLevel::Unknown as u8) > (LogLevel::Debug as u8));
        assert_eq!(LogLevel::Unknown.name(), "unknown");
    }

    #[test]
    fn extracting_json_fields_finds_them_and_says_so_when_it_cannot() {
        let line = r#"{"msg":"hello","ts":12345}"#;
        assert_eq!(extract_json_string(line, "msg").as_deref(), Some("hello"));
        assert_eq!(extract_json_number(line, "ts"), Some(12345));
        assert_eq!(extract_json_string(line, "absent"), None);
        assert_eq!(extract_json_number(line, "absent"), None);
    }

    /// The host has no Slate kernel, so the ring read declines and the file
    /// fallbacks are what run.
    ///
    /// This is the property the 2026-09-10 change turns on: reading used to go
    /// to `/proc/kmsg`, which this kernel serves no entry for, so the primary
    /// source silently never worked and the fallbacks were doing all the work.
    /// Now the primary source is `klogctl`, and on a host without one the
    /// decline is explicit rather than a failed open.
    #[cfg(not(unix))]
    #[test]
    fn the_host_has_no_kernel_ring_to_read() {
        assert!(read_kernel_ring().is_none());
        assert_eq!(libcall::klog_clear(), Err(libcall::ENOSYS));
    }
}
