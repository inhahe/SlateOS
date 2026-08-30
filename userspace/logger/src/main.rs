//! logger — send messages to the system log for Slate OS
//!
//! Compatible with POSIX/BSD logger(1). Writes log entries to
//! the system log via /dev/log socket or direct file append.

use std::env;
use std::fs::{self, OpenOptions};
use std::io::{self, BufRead, Write};
use std::process;
use std::time::{SystemTime, UNIX_EPOCH};

// ── Syslog facilities ────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(u8)]
enum Facility {
    Kern = 0,
    User = 1,
    Mail = 2,
    Daemon = 3,
    Auth = 4,
    Syslog = 5,
    Lpr = 6,
    News = 7,
    Uucp = 8,
    Cron = 9,
    Authpriv = 10,
    Ftp = 11,
    Local0 = 16,
    Local1 = 17,
    Local2 = 18,
    Local3 = 19,
    Local4 = 20,
    Local5 = 21,
    Local6 = 22,
    Local7 = 23,
}

impl Facility {
    fn from_name(name: &str) -> Option<Self> {
        match name.to_lowercase().as_str() {
            "kern" | "kernel" => Some(Self::Kern),
            "user" => Some(Self::User),
            "mail" => Some(Self::Mail),
            "daemon" => Some(Self::Daemon),
            "auth" | "security" => Some(Self::Auth),
            "syslog" => Some(Self::Syslog),
            "lpr" => Some(Self::Lpr),
            "news" => Some(Self::News),
            "uucp" => Some(Self::Uucp),
            "cron" => Some(Self::Cron),
            "authpriv" => Some(Self::Authpriv),
            "ftp" => Some(Self::Ftp),
            "local0" => Some(Self::Local0),
            "local1" => Some(Self::Local1),
            "local2" => Some(Self::Local2),
            "local3" => Some(Self::Local3),
            "local4" => Some(Self::Local4),
            "local5" => Some(Self::Local5),
            "local6" => Some(Self::Local6),
            "local7" => Some(Self::Local7),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Kern => "kern",
            Self::User => "user",
            Self::Mail => "mail",
            Self::Daemon => "daemon",
            Self::Auth => "auth",
            Self::Syslog => "syslog",
            Self::Lpr => "lpr",
            Self::News => "news",
            Self::Uucp => "uucp",
            Self::Cron => "cron",
            Self::Authpriv => "authpriv",
            Self::Ftp => "ftp",
            Self::Local0 => "local0",
            Self::Local1 => "local1",
            Self::Local2 => "local2",
            Self::Local3 => "local3",
            Self::Local4 => "local4",
            Self::Local5 => "local5",
            Self::Local6 => "local6",
            Self::Local7 => "local7",
        }
    }
}

// ── Syslog severities ────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(u8)]
enum Severity {
    Emerg = 0,
    Alert = 1,
    Crit = 2,
    Err = 3,
    Warning = 4,
    Notice = 5,
    Info = 6,
    Debug = 7,
}

impl Severity {
    fn from_name(name: &str) -> Option<Self> {
        match name.to_lowercase().as_str() {
            "emerg" | "panic" => Some(Self::Emerg),
            "alert" => Some(Self::Alert),
            "crit" | "critical" => Some(Self::Crit),
            "err" | "error" => Some(Self::Err),
            "warning" | "warn" => Some(Self::Warning),
            "notice" => Some(Self::Notice),
            "info" => Some(Self::Info),
            "debug" => Some(Self::Debug),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Emerg => "emerg",
            Self::Alert => "alert",
            Self::Crit => "crit",
            Self::Err => "err",
            Self::Warning => "warning",
            Self::Notice => "notice",
            Self::Info => "info",
            Self::Debug => "debug",
        }
    }
}

// ── Priority parsing ─────────────────────────────────────────────

/// Parse a priority string like "user.info" or numeric priority
fn parse_priority(s: &str) -> Option<(Facility, Severity)> {
    // Try numeric first
    if let Ok(n) = s.parse::<u32>() {
        let facility_num = (n >> 3) as u8;
        let severity_num = (n & 7) as u8;
        let facility = match facility_num {
            0 => Facility::Kern,
            1 => Facility::User,
            2 => Facility::Mail,
            3 => Facility::Daemon,
            4 => Facility::Auth,
            5 => Facility::Syslog,
            6 => Facility::Lpr,
            7 => Facility::News,
            8 => Facility::Uucp,
            9 => Facility::Cron,
            10 => Facility::Authpriv,
            11 => Facility::Ftp,
            16 => Facility::Local0,
            17 => Facility::Local1,
            18 => Facility::Local2,
            19 => Facility::Local3,
            20 => Facility::Local4,
            21 => Facility::Local5,
            22 => Facility::Local6,
            23 => Facility::Local7,
            _ => return None,
        };
        let severity = match severity_num {
            0 => Severity::Emerg,
            1 => Severity::Alert,
            2 => Severity::Crit,
            3 => Severity::Err,
            4 => Severity::Warning,
            5 => Severity::Notice,
            6 => Severity::Info,
            7 => Severity::Debug,
            _ => return None,
        };
        return Some((facility, severity));
    }

    // Try facility.severity
    if let Some(dot_pos) = s.find('.') {
        let fac_name = &s[..dot_pos];
        let sev_name = &s[dot_pos + 1..];
        let facility = Facility::from_name(fac_name)?;
        let severity = Severity::from_name(sev_name)?;
        return Some((facility, severity));
    }

    // Try just severity (assume user facility)
    if let Some(severity) = Severity::from_name(s) {
        return Some((Facility::User, severity));
    }

    None
}

// ── Timestamp formatting ─────────────────────────────────────────

fn format_timestamp() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    // Convert epoch seconds to broken-down time (simplified UTC)
    let days = secs / 86400;
    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    // Calculate month and day from days since epoch (1970-01-01)
    let (year, month, day) = days_to_date(days);
    let _ = year; // We only need month and day for syslog format

    let month_name = match month {
        1 => "Jan",
        2 => "Feb",
        3 => "Mar",
        4 => "Apr",
        5 => "May",
        6 => "Jun",
        7 => "Jul",
        8 => "Aug",
        9 => "Sep",
        10 => "Oct",
        11 => "Nov",
        12 => "Dec",
        _ => "???",
    };

    format!(
        "{} {:>2} {:02}:{:02}:{:02}",
        month_name, day, hours, minutes, seconds
    )
}

fn days_to_date(days_since_epoch: u64) -> (u64, u32, u32) {
    // Civil days algorithm
    let z = days_since_epoch + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m as u32, d as u32)
}

fn format_rfc3339() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let days = secs / 86400;
    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    let (year, month, day) = days_to_date(days);

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year, month, day, hours, minutes, seconds
    )
}

// ── JSON-lines format ────────────────────────────────────────────

fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out
}

// ── Options ──────────────────────────────────────────────────────

struct Options {
    tag: Option<String>,
    priority: (Facility, Severity),
    log_file: String,
    stderr: bool,
    id: bool,
    pid_override: Option<u32>,
    socket: Option<String>,
    rfc3339: bool,
    json: bool,
    size_limit: Option<usize>,
    message_parts: Vec<String>,
    read_stdin: bool,
}

fn print_help() {
    println!("Usage: logger [OPTIONS] [MESSAGE...]");
    println!();
    println!("Write messages to the system log.");
    println!();
    println!("Options:");
    println!("  -p, --priority PRIORITY   specify priority (facility.severity or numeric)");
    println!("                            default: user.notice");
    println!("  -t, --tag TAG             mark message with TAG (default: username)");
    println!("  -i, --id                  log the process ID with each line");
    println!("  -f, --file FILE           log the contents of FILE");
    println!("  -s, --stderr              output to stderr as well as syslog");
    println!("  -u, --socket SOCKET       write to SOCKET instead of /dev/log");
    println!("  -n, --server HOST         ignored (compatibility)");
    println!("  -P, --port PORT           ignored (compatibility)");
    println!("  --rfc3339                 use RFC 3339 timestamp format");
    println!("  --json                    output in JSON-lines format");
    println!("  --size SIZE               max message size in bytes (default: 1024)");
    println!("  --pid PID                 override PID in log entry");
    println!("  -h, --help                display this help and exit");
    println!("  --version                 output version information and exit");
    println!();
    println!("If no MESSAGE is given, standard input is logged line by line.");
    println!();
    println!("Priority format: facility.severity (e.g., user.info, daemon.err)");
    println!();
    println!("Facilities: kern, user, mail, daemon, auth, syslog, lpr, news,");
    println!("            uucp, cron, authpriv, ftp, local0-local7");
    println!();
    println!("Severities: emerg, alert, crit, err, warning, notice, info, debug");
}

fn parse_args(args: &[String]) -> Options {
    let mut opts = Options {
        tag: None,
        priority: (Facility::User, Severity::Notice),
        log_file: "/var/log/syslog".to_string(),
        stderr: false,
        id: false,
        pid_override: None,
        socket: None,
        rfc3339: false,
        json: false,
        size_limit: Some(1024),
        message_parts: Vec::new(),
        read_stdin: false,
    };

    let mut i = 0;
    let mut file_to_log: Option<String> = None;

    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            "-p" | "--priority" => {
                i += 1;
                if i < args.len() {
                    match parse_priority(&args[i]) {
                        Some(p) => opts.priority = p,
                        None => {
                            eprintln!("logger: unknown priority: {}", args[i]);
                            process::exit(1);
                        }
                    }
                }
            }
            "-t" | "--tag" => {
                i += 1;
                if i < args.len() {
                    opts.tag = Some(args[i].clone());
                }
            }
            "-i" | "--id" => {
                opts.id = true;
            }
            "-f" | "--file" => {
                i += 1;
                if i < args.len() {
                    file_to_log = Some(args[i].clone());
                }
            }
            "-s" | "--stderr" => {
                opts.stderr = true;
            }
            "-u" | "--socket" => {
                i += 1;
                if i < args.len() {
                    opts.socket = Some(args[i].clone());
                }
            }
            "-n" | "--server" | "-P" | "--port" => {
                // Compatibility: skip the argument
                i += 1;
            }
            "--rfc3339" => {
                opts.rfc3339 = true;
            }
            "--json" => {
                opts.json = true;
            }
            "--size" => {
                i += 1;
                if i < args.len()
                    && let Ok(n) = args[i].parse::<usize>() {
                        opts.size_limit = Some(n);
                    }
            }
            "--pid" => {
                i += 1;
                if i < args.len()
                    && let Ok(pid) = args[i].parse::<u32>() {
                        opts.pid_override = Some(pid);
                    }
            }
            "-h" | "--help" => {
                print_help();
                process::exit(0);
            }
            "--version" => {
                println!("logger (Slate OS) 0.1.0");
                process::exit(0);
            }
            _ if arg.starts_with("--priority=") => {
                let val = arg.strip_prefix("--priority=").unwrap_or("");
                match parse_priority(val) {
                    Some(p) => opts.priority = p,
                    None => {
                        eprintln!("logger: unknown priority: {}", val);
                        process::exit(1);
                    }
                }
            }
            _ if arg.starts_with("--tag=") => {
                opts.tag = Some(arg.strip_prefix("--tag=").unwrap_or("").to_string());
            }
            _ if arg.starts_with("--socket=") => {
                opts.socket = Some(arg.strip_prefix("--socket=").unwrap_or("").to_string());
            }
            _ if arg.starts_with("--size=") => {
                let val = arg.strip_prefix("--size=").unwrap_or("");
                if let Ok(n) = val.parse::<usize>() {
                    opts.size_limit = Some(n);
                }
            }
            _ if arg.starts_with("--pid=") => {
                let val = arg.strip_prefix("--pid=").unwrap_or("");
                if let Ok(pid) = val.parse::<u32>() {
                    opts.pid_override = Some(pid);
                }
            }
            _ if arg.starts_with('-') && arg.len() > 1 && !arg.starts_with("--") => {
                // Handle combined short flags
                let chars: Vec<char> = arg[1..].chars().collect();
                let mut j = 0;
                while j < chars.len() {
                    match chars[j] {
                        'i' => opts.id = true,
                        's' => opts.stderr = true,
                        'p' => {
                            i += 1;
                            if i < args.len() {
                                match parse_priority(&args[i]) {
                                    Some(p) => opts.priority = p,
                                    None => {
                                        eprintln!("logger: unknown priority: {}", args[i]);
                                        process::exit(1);
                                    }
                                }
                            }
                        }
                        't' => {
                            i += 1;
                            if i < args.len() {
                                opts.tag = Some(args[i].clone());
                            }
                        }
                        'f' => {
                            i += 1;
                            if i < args.len() {
                                file_to_log = Some(args[i].clone());
                            }
                        }
                        'u' => {
                            i += 1;
                            if i < args.len() {
                                opts.socket = Some(args[i].clone());
                            }
                        }
                        'h' => {
                            print_help();
                            process::exit(0);
                        }
                        _ => {
                            eprintln!("logger: unknown option '-{}'", chars[j]);
                            process::exit(1);
                        }
                    }
                    j += 1;
                }
            }
            _ => {
                opts.message_parts.push(arg.clone());
            }
        }
        i += 1;
    }

    // If -f was given, read that file's contents as messages
    if let Some(file_path) = file_to_log {
        match fs::read_to_string(&file_path) {
            Ok(content) => {
                for line in content.lines() {
                    if !line.is_empty() {
                        opts.message_parts.push(line.to_string());
                    }
                }
            }
            Err(e) => {
                eprintln!("logger: {}: {}", file_path, e);
                process::exit(1);
            }
        }
    }

    // If no message parts, read from stdin
    if opts.message_parts.is_empty() {
        opts.read_stdin = true;
    }

    opts
}

// ── Log entry formatting ─────────────────────────────────────────

fn get_hostname() -> String {
    // Try /etc/hostname, fall back to "localhost"
    fs::read_to_string("/etc/hostname")
        .ok()
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "localhost".to_string())
}

fn get_username() -> String {
    env::var("USER")
        .or_else(|_| env::var("LOGNAME"))
        .unwrap_or_else(|_| "root".to_string())
}

fn get_pid() -> u32 {
    // Read /proc/self/stat for PID
    if let Ok(stat) = fs::read_to_string("/proc/self/stat")
        && let Some(pid_str) = stat.split_whitespace().next()
            && let Ok(pid) = pid_str.parse::<u32>() {
                return pid;
            }
    // Fallback
    0
}

fn format_syslog_entry(
    opts: &Options,
    message: &str,
    hostname: &str,
    tag: &str,
    pid: u32,
) -> String {
    let pri = (opts.priority.0 as u32) * 8 + (opts.priority.1 as u32);

    let timestamp = if opts.rfc3339 {
        format_rfc3339()
    } else {
        format_timestamp()
    };

    let pid_part = if opts.id {
        format!("[{}]", opts.pid_override.unwrap_or(pid))
    } else {
        String::new()
    };

    let msg = if let Some(limit) = opts.size_limit {
        if message.len() > limit {
            &message[..limit]
        } else {
            message
        }
    } else {
        message
    };

    format!("<{}>{} {} {}{}: {}", pri, timestamp, hostname, tag, pid_part, msg)
}

fn format_json_entry(
    opts: &Options,
    message: &str,
    hostname: &str,
    tag: &str,
    pid: u32,
) -> String {
    let pri = (opts.priority.0 as u32) * 8 + (opts.priority.1 as u32);

    let msg = if let Some(limit) = opts.size_limit {
        if message.len() > limit {
            &message[..limit]
        } else {
            message
        }
    } else {
        message
    };

    let mut json = String::with_capacity(256);
    json.push_str("{\"timestamp\":\"");
    json.push_str(&json_escape(&format_rfc3339()));
    json.push_str("\",\"hostname\":\"");
    json.push_str(&json_escape(hostname));
    json.push_str("\",\"facility\":\"");
    json.push_str(opts.priority.0.name());
    json.push_str("\",\"severity\":\"");
    json.push_str(opts.priority.1.name());
    json.push_str("\",\"priority\":");
    json.push_str(&pri.to_string());
    json.push_str(",\"tag\":\"");
    json.push_str(&json_escape(tag));
    json.push('"');
    if opts.id {
        json.push_str(",\"pid\":");
        json.push_str(&opts.pid_override.unwrap_or(pid).to_string());
    }
    json.push_str(",\"message\":\"");
    json.push_str(&json_escape(msg));
    json.push_str("\"}");

    json
}

// ── Log writing ──────────────────────────────────────────────────

fn write_log_entry(opts: &Options, entry: &str) {
    // Try writing to log file
    if let Ok(mut file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&opts.log_file)
    {
        let _ = writeln!(file, "{}", entry);
    } else {
        // If we can't write to the log file, output to stdout as fallback
        println!("{}", entry);
    }

    // Also output to stderr if -s flag is set
    if opts.stderr {
        eprintln!("{}", entry);
    }
}

fn log_message(opts: &Options, message: &str, hostname: &str, tag: &str, pid: u32) {
    let entry = if opts.json {
        format_json_entry(opts, message, hostname, tag, pid)
    } else {
        format_syslog_entry(opts, message, hostname, tag, pid)
    };

    write_log_entry(opts, &entry);
}

// ── main ─────────────────────────────────────────────────────────

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let opts = parse_args(&args);

    let hostname = get_hostname();
    let tag = opts
        .tag
        .clone()
        .unwrap_or_else(get_username);
    let pid = get_pid();

    if opts.read_stdin {
        // Log each line from stdin
        let stdin = io::stdin();
        for line in stdin.lock().lines() {
            match line {
                Ok(msg) => {
                    if !msg.is_empty() {
                        log_message(&opts, &msg, &hostname, &tag, pid);
                    }
                }
                Err(e) => {
                    eprintln!("logger: read error: {}", e);
                    process::exit(1);
                }
            }
        }
    } else {
        // Log the command-line message
        let message = opts.message_parts.join(" ");
        log_message(&opts, &message, &hostname, &tag, pid);
    }
}

// ── Tests ────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // Priority parsing
    #[test]
    fn test_parse_priority_named() {
        let (fac, sev) = parse_priority("user.info").unwrap();
        assert_eq!(fac, Facility::User);
        assert_eq!(sev, Severity::Info);
    }

    #[test]
    fn test_parse_priority_numeric() {
        // user (1) * 8 + info (6) = 14
        let (fac, sev) = parse_priority("14").unwrap();
        assert_eq!(fac, Facility::User);
        assert_eq!(sev, Severity::Info);
    }

    #[test]
    fn test_parse_priority_daemon_err() {
        let (fac, sev) = parse_priority("daemon.err").unwrap();
        assert_eq!(fac, Facility::Daemon);
        assert_eq!(sev, Severity::Err);
    }

    #[test]
    fn test_parse_priority_kern_emerg() {
        let (fac, sev) = parse_priority("kern.emerg").unwrap();
        assert_eq!(fac, Facility::Kern);
        assert_eq!(sev, Severity::Emerg);
    }

    #[test]
    fn test_parse_priority_local7_debug() {
        let (fac, sev) = parse_priority("local7.debug").unwrap();
        assert_eq!(fac, Facility::Local7);
        assert_eq!(sev, Severity::Debug);
    }

    #[test]
    fn test_parse_priority_severity_only() {
        let (fac, sev) = parse_priority("err").unwrap();
        assert_eq!(fac, Facility::User);
        assert_eq!(sev, Severity::Err);
    }

    #[test]
    fn test_parse_priority_invalid() {
        assert!(parse_priority("invalid.bogus").is_none());
    }

    #[test]
    fn test_parse_priority_numeric_zero() {
        let (fac, sev) = parse_priority("0").unwrap();
        assert_eq!(fac, Facility::Kern);
        assert_eq!(sev, Severity::Emerg);
    }

    #[test]
    fn test_parse_priority_auth_crit() {
        // auth (4) * 8 + crit (2) = 34
        let (fac, sev) = parse_priority("34").unwrap();
        assert_eq!(fac, Facility::Auth);
        assert_eq!(sev, Severity::Crit);
    }

    // Facility names
    #[test]
    fn test_facility_names() {
        assert_eq!(Facility::from_name("kern"), Some(Facility::Kern));
        assert_eq!(Facility::from_name("kernel"), Some(Facility::Kern));
        assert_eq!(Facility::from_name("mail"), Some(Facility::Mail));
        assert_eq!(Facility::from_name("cron"), Some(Facility::Cron));
        assert_eq!(Facility::from_name("local0"), Some(Facility::Local0));
        assert_eq!(Facility::from_name("bogus"), None);
    }

    // Severity names
    #[test]
    fn test_severity_names() {
        assert_eq!(Severity::from_name("emerg"), Some(Severity::Emerg));
        assert_eq!(Severity::from_name("panic"), Some(Severity::Emerg));
        assert_eq!(Severity::from_name("warn"), Some(Severity::Warning));
        assert_eq!(Severity::from_name("warning"), Some(Severity::Warning));
        assert_eq!(Severity::from_name("error"), Some(Severity::Err));
        assert_eq!(Severity::from_name("bogus"), None);
    }

    // Facility display names
    #[test]
    fn test_facility_display() {
        assert_eq!(Facility::Kern.name(), "kern");
        assert_eq!(Facility::User.name(), "user");
        assert_eq!(Facility::Local7.name(), "local7");
    }

    // Severity display names
    #[test]
    fn test_severity_display() {
        assert_eq!(Severity::Emerg.name(), "emerg");
        assert_eq!(Severity::Info.name(), "info");
        assert_eq!(Severity::Debug.name(), "debug");
    }

    // Timestamp formatting
    #[test]
    fn test_format_timestamp_not_empty() {
        let ts = format_timestamp();
        assert!(!ts.is_empty());
        // Should be like "May 18 12:34:56"
        assert!(ts.len() >= 14);
    }

    #[test]
    fn test_format_rfc3339_not_empty() {
        let ts = format_rfc3339();
        assert!(!ts.is_empty());
        assert!(ts.contains('T'));
        assert!(ts.ends_with('Z'));
    }

    // Date conversion
    #[test]
    fn test_days_to_date_epoch() {
        let (y, m, d) = days_to_date(0);
        assert_eq!((y, m, d), (1970, 1, 1));
    }

    #[test]
    fn test_days_to_date_known() {
        // 2024-01-01 = day 19723
        let (y, m, d) = days_to_date(19723);
        assert_eq!((y, m, d), (2024, 1, 1));
    }

    #[test]
    fn test_days_to_date_leap_year() {
        // 2024-02-29 = day 19782
        let (y, m, d) = days_to_date(19782);
        assert_eq!(y, 2024);
        assert_eq!(m, 2);
        assert_eq!(d, 29);
    }

    // JSON escaping
    #[test]
    fn test_json_escape_simple() {
        assert_eq!(json_escape("hello"), "hello");
    }

    #[test]
    fn test_json_escape_quotes() {
        assert_eq!(json_escape("say \"hi\""), "say \\\"hi\\\"");
    }

    #[test]
    fn test_json_escape_backslash() {
        assert_eq!(json_escape("a\\b"), "a\\\\b");
    }

    #[test]
    fn test_json_escape_newline() {
        assert_eq!(json_escape("line1\nline2"), "line1\\nline2");
    }

    #[test]
    fn test_json_escape_tab() {
        assert_eq!(json_escape("a\tb"), "a\\tb");
    }

    #[test]
    fn test_json_escape_control_char() {
        let s = String::from_utf8(vec![0x01]).unwrap();
        assert_eq!(json_escape(&s), "\\u0001");
    }

    // Syslog entry formatting
    #[test]
    fn test_format_syslog_basic() {
        let opts = Options {
            tag: None,
            priority: (Facility::User, Severity::Notice),
            log_file: "/var/log/syslog".to_string(),
            stderr: false,
            id: false,
            pid_override: None,
            socket: None,
            rfc3339: false,
            json: false,
            size_limit: Some(1024),
            message_parts: Vec::new(),
            read_stdin: false,
        };
        let entry = format_syslog_entry(&opts, "test message", "myhost", "mytag", 1234);
        // Priority: user(1)*8 + notice(5) = 13
        assert!(entry.starts_with("<13>"));
        assert!(entry.contains("myhost"));
        assert!(entry.contains("mytag"));
        assert!(entry.contains("test message"));
    }

    #[test]
    fn test_format_syslog_with_pid() {
        let opts = Options {
            tag: None,
            priority: (Facility::Daemon, Severity::Err),
            log_file: "/var/log/syslog".to_string(),
            stderr: false,
            id: true,
            pid_override: None,
            socket: None,
            rfc3339: false,
            json: false,
            size_limit: Some(1024),
            message_parts: Vec::new(),
            read_stdin: false,
        };
        let entry = format_syslog_entry(&opts, "error", "host", "daemon", 5678);
        // Priority: daemon(3)*8 + err(3) = 27
        assert!(entry.starts_with("<27>"));
        assert!(entry.contains("[5678]"));
    }

    #[test]
    fn test_format_syslog_pid_override() {
        let opts = Options {
            tag: None,
            priority: (Facility::User, Severity::Info),
            log_file: "/var/log/syslog".to_string(),
            stderr: false,
            id: true,
            pid_override: Some(9999),
            socket: None,
            rfc3339: false,
            json: false,
            size_limit: Some(1024),
            message_parts: Vec::new(),
            read_stdin: false,
        };
        let entry = format_syslog_entry(&opts, "msg", "host", "tag", 1111);
        assert!(entry.contains("[9999]"));
        assert!(!entry.contains("[1111]"));
    }

    #[test]
    fn test_format_syslog_size_limit() {
        let opts = Options {
            tag: None,
            priority: (Facility::User, Severity::Info),
            log_file: "/var/log/syslog".to_string(),
            stderr: false,
            id: false,
            pid_override: None,
            socket: None,
            rfc3339: false,
            json: false,
            size_limit: Some(10),
            message_parts: Vec::new(),
            read_stdin: false,
        };
        let entry = format_syslog_entry(&opts, "this is a very long message", "h", "t", 0);
        assert!(entry.contains("this is a "));
        assert!(!entry.contains("very long"));
    }

    // JSON entry formatting
    #[test]
    fn test_format_json_basic() {
        let opts = Options {
            tag: None,
            priority: (Facility::User, Severity::Info),
            log_file: "/var/log/syslog".to_string(),
            stderr: false,
            id: false,
            pid_override: None,
            socket: None,
            rfc3339: false,
            json: true,
            size_limit: Some(1024),
            message_parts: Vec::new(),
            read_stdin: false,
        };
        let entry = format_json_entry(&opts, "test msg", "myhost", "mytag", 42);
        assert!(entry.starts_with('{'));
        assert!(entry.ends_with('}'));
        assert!(entry.contains("\"facility\":\"user\""));
        assert!(entry.contains("\"severity\":\"info\""));
        assert!(entry.contains("\"message\":\"test msg\""));
        assert!(entry.contains("\"hostname\":\"myhost\""));
        assert!(entry.contains("\"tag\":\"mytag\""));
        // priority: user(1)*8 + info(6) = 14
        assert!(entry.contains("\"priority\":14"));
    }

    #[test]
    fn test_format_json_with_pid() {
        let opts = Options {
            tag: None,
            priority: (Facility::User, Severity::Info),
            log_file: "/var/log/syslog".to_string(),
            stderr: false,
            id: true,
            pid_override: None,
            socket: None,
            rfc3339: false,
            json: true,
            size_limit: Some(1024),
            message_parts: Vec::new(),
            read_stdin: false,
        };
        let entry = format_json_entry(&opts, "msg", "h", "t", 42);
        assert!(entry.contains("\"pid\":42"));
    }

    // Argument parsing
    #[test]
    fn test_parse_args_defaults() {
        let opts = parse_args(&[]);
        assert_eq!(opts.priority, (Facility::User, Severity::Notice));
        assert!(opts.tag.is_none());
        assert!(!opts.id);
        assert!(!opts.stderr);
        assert!(!opts.rfc3339);
        assert!(!opts.json);
        assert!(opts.read_stdin);
    }

    #[test]
    fn test_parse_args_priority() {
        let args = vec!["-p".to_string(), "daemon.err".to_string(), "msg".to_string()];
        let opts = parse_args(&args);
        assert_eq!(opts.priority, (Facility::Daemon, Severity::Err));
        assert_eq!(opts.message_parts, vec!["msg"]);
    }

    #[test]
    fn test_parse_args_tag() {
        let args = vec!["-t".to_string(), "myapp".to_string(), "hello".to_string()];
        let opts = parse_args(&args);
        assert_eq!(opts.tag, Some("myapp".to_string()));
    }

    #[test]
    fn test_parse_args_flags() {
        let args = vec!["-is".to_string(), "test".to_string()];
        let opts = parse_args(&args);
        assert!(opts.id);
        assert!(opts.stderr);
    }

    #[test]
    fn test_parse_args_rfc3339() {
        let args = vec!["--rfc3339".to_string(), "test".to_string()];
        let opts = parse_args(&args);
        assert!(opts.rfc3339);
    }

    #[test]
    fn test_parse_args_json() {
        let args = vec!["--json".to_string(), "test".to_string()];
        let opts = parse_args(&args);
        assert!(opts.json);
    }

    #[test]
    fn test_parse_args_message() {
        let args = vec!["hello".to_string(), "world".to_string()];
        let opts = parse_args(&args);
        assert_eq!(opts.message_parts, vec!["hello", "world"]);
        assert!(!opts.read_stdin);
    }
}
