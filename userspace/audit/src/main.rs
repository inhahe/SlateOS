//! OurOS audit framework utilities.
//!
//! Multi-personality binary providing:
//! - **auditctl** (default) — audit rule management
//! - **auditd** — audit daemon
//! - **ausearch** — search audit logs
//! - **aureport** — audit report generator
//! - **autrace** — trace process using audit
//!
//! Implements the Linux audit framework interface for system auditing,
//! security monitoring, and compliance logging.

#![deny(clippy::all)]

use std::collections::HashMap;
use std::env;
use std::fmt;
use std::fs;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::process;

const VERSION: &str = "0.1.0";
const DEFAULT_LOG_FILE: &str = "/var/log/audit/audit.log";
const DEFAULT_CONFIG_FILE: &str = "/etc/audit/auditd.conf";
const DEFAULT_PID_FILE: &str = "/var/run/auditd.pid";

// ============================================================================
// Audit event types
// ============================================================================

/// Known audit message types.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum MessageType {
    Syscall,
    Path,
    Cwd,
    Execve,
    Proctitle,
    Config,
    UserAuth,
    UserLogin,
    UserAcct,
    UserStart,
    UserEnd,
    CredAcq,
    CredDisp,
    CredRefr,
    UserCmd,
    Login,
    Avc,
    SelinuxErr,
    DaemonStart,
    DaemonEnd,
    DaemonAbort,
    DaemonConfig,
    ServiceStart,
    ServiceStop,
    Unknown,
}

impl MessageType {
    fn from_str(s: &str) -> Self {
        match s.to_uppercase().as_str() {
            "SYSCALL" => Self::Syscall,
            "PATH" => Self::Path,
            "CWD" => Self::Cwd,
            "EXECVE" => Self::Execve,
            "PROCTITLE" => Self::Proctitle,
            "CONFIG_CHANGE" | "CONFIG" => Self::Config,
            "USER_AUTH" => Self::UserAuth,
            "USER_LOGIN" => Self::UserLogin,
            "USER_ACCT" => Self::UserAcct,
            "USER_START" => Self::UserStart,
            "USER_END" => Self::UserEnd,
            "CRED_ACQ" => Self::CredAcq,
            "CRED_DISP" => Self::CredDisp,
            "CRED_REFR" => Self::CredRefr,
            "USER_CMD" => Self::UserCmd,
            "LOGIN" => Self::Login,
            "AVC" => Self::Avc,
            "SELINUX_ERR" => Self::SelinuxErr,
            "DAEMON_START" => Self::DaemonStart,
            "DAEMON_END" => Self::DaemonEnd,
            "DAEMON_ABORT" => Self::DaemonAbort,
            "DAEMON_CONFIG" => Self::DaemonConfig,
            "SERVICE_START" => Self::ServiceStart,
            "SERVICE_STOP" => Self::ServiceStop,
            _ => Self::Unknown,
        }
    }

    fn as_str(&self) -> &'static str {
        match self {
            Self::Syscall => "SYSCALL",
            Self::Path => "PATH",
            Self::Cwd => "CWD",
            Self::Execve => "EXECVE",
            Self::Proctitle => "PROCTITLE",
            Self::Config => "CONFIG_CHANGE",
            Self::UserAuth => "USER_AUTH",
            Self::UserLogin => "USER_LOGIN",
            Self::UserAcct => "USER_ACCT",
            Self::UserStart => "USER_START",
            Self::UserEnd => "USER_END",
            Self::CredAcq => "CRED_ACQ",
            Self::CredDisp => "CRED_DISP",
            Self::CredRefr => "CRED_REFR",
            Self::UserCmd => "USER_CMD",
            Self::Login => "LOGIN",
            Self::Avc => "AVC",
            Self::SelinuxErr => "SELINUX_ERR",
            Self::DaemonStart => "DAEMON_START",
            Self::DaemonEnd => "DAEMON_END",
            Self::DaemonAbort => "DAEMON_ABORT",
            Self::DaemonConfig => "DAEMON_CONFIG",
            Self::ServiceStart => "SERVICE_START",
            Self::ServiceStop => "SERVICE_STOP",
            Self::Unknown => "UNKNOWN",
        }
    }

    fn is_auth_related(&self) -> bool {
        matches!(
            self,
            Self::UserAuth | Self::CredAcq | Self::CredDisp | Self::CredRefr
        )
    }

    fn is_login_related(&self) -> bool {
        matches!(self, Self::UserLogin | Self::Login | Self::UserStart | Self::UserEnd)
    }
}

impl fmt::Display for MessageType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

// ============================================================================
// Audit rule action/filter types
// ============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RuleAction {
    Always,
    Never,
}

impl RuleAction {
    fn from_str(s: &str) -> Option<Self> {
        match s {
            "always" => Some(Self::Always),
            "never" => Some(Self::Never),
            _ => None,
        }
    }

    fn as_str(&self) -> &'static str {
        match self {
            Self::Always => "always",
            Self::Never => "never",
        }
    }
}

impl fmt::Display for RuleAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RuleFilter {
    Task,
    Exit,
    User,
    Exclude,
}

impl RuleFilter {
    fn from_str(s: &str) -> Option<Self> {
        match s {
            "task" => Some(Self::Task),
            "exit" => Some(Self::Exit),
            "user" => Some(Self::User),
            "exclude" => Some(Self::Exclude),
            _ => None,
        }
    }

    fn as_str(&self) -> &'static str {
        match self {
            Self::Task => "task",
            Self::Exit => "exit",
            Self::User => "user",
            Self::Exclude => "exclude",
        }
    }
}

impl fmt::Display for RuleFilter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

// ============================================================================
// File watch permissions
// ============================================================================

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct WatchPerms {
    read: bool,
    write: bool,
    execute: bool,
    attribute: bool,
}

impl WatchPerms {
    fn from_str(s: &str) -> Option<Self> {
        let mut p = Self::default();
        for c in s.chars() {
            match c {
                'r' => p.read = true,
                'w' => p.write = true,
                'x' => p.execute = true,
                'a' => p.attribute = true,
                _ => return None,
            }
        }
        Some(p)
    }

    fn as_string(&self) -> String {
        let mut s = String::new();
        if self.read {
            s.push('r');
        }
        if self.write {
            s.push('w');
        }
        if self.execute {
            s.push('x');
        }
        if self.attribute {
            s.push('a');
        }
        s
    }
}

impl fmt::Display for WatchPerms {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_string())
    }
}

// ============================================================================
// Audit rules
// ============================================================================

#[derive(Clone, Debug, PartialEq, Eq)]
struct FieldFilter {
    field: String,
    op: String,
    value: String,
}

impl fmt::Display for FieldFilter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}{}",self.field, self.op, self.value)
    }
}

#[derive(Clone, Debug, PartialEq)]
enum AuditRule {
    /// File/directory watch rule.
    Watch {
        path: String,
        perms: WatchPerms,
        key: Option<String>,
    },
    /// Syscall-based rule.
    Syscall {
        action: RuleAction,
        filter: RuleFilter,
        syscall: Option<String>,
        fields: Vec<FieldFilter>,
        key: Option<String>,
    },
}

impl fmt::Display for AuditRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Watch { path, perms, key } => {
                write!(f, "-w {} -p {}", path, perms)?;
                if let Some(k) = key {
                    write!(f, " -k {}", k)?;
                }
                Ok(())
            }
            Self::Syscall {
                action,
                filter,
                syscall,
                fields,
                key,
            } => {
                write!(f, "-a {},{}", action, filter)?;
                if let Some(sc) = syscall {
                    write!(f, " -S {}", sc)?;
                }
                for field in fields {
                    write!(f, " -F {}", field)?;
                }
                if let Some(k) = key {
                    write!(f, " -k {}", k)?;
                }
                Ok(())
            }
        }
    }
}

// ============================================================================
// Audit status
// ============================================================================

#[derive(Clone, Debug)]
struct AuditStatus {
    enabled: u32,
    pid: u32,
    rate_limit: u32,
    backlog_limit: u32,
    lost: u32,
    backlog: u32,
    failure_mode: u32,
}

impl AuditStatus {
    fn new() -> Self {
        Self {
            enabled: 1,
            pid: std::process::id(),
            rate_limit: 0,
            backlog_limit: 8192,
            lost: 0,
            backlog: 0,
            failure_mode: 1,
        }
    }
}

impl fmt::Display for AuditStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "enabled {}", self.enabled)?;
        writeln!(f, "failure {}", self.failure_mode)?;
        writeln!(f, "pid {}", self.pid)?;
        writeln!(f, "rate_limit {}", self.rate_limit)?;
        writeln!(f, "backlog_limit {}", self.backlog_limit)?;
        writeln!(f, "lost {}", self.lost)?;
        write!(f, "backlog {}", self.backlog)
    }
}

// ============================================================================
// Audit log record
// ============================================================================

/// A parsed audit log record.
#[derive(Clone, Debug)]
struct AuditRecord {
    msg_type: MessageType,
    timestamp: f64,
    serial: u64,
    fields: HashMap<String, String>,
    raw_line: String,
}

impl AuditRecord {
    /// Parse a line in audit log format:
    /// type=X msg=audit(timestamp:serial): field=value field=value ...
    fn parse(line: &str) -> Option<Self> {
        let line = line.trim();
        if line.is_empty() {
            return None;
        }

        // Extract type= prefix
        let rest = line.strip_prefix("type=")?;
        let space_idx = rest.find(' ')?;
        let type_str = &rest[..space_idx];
        let msg_type = MessageType::from_str(type_str);

        let rest = &rest[space_idx + 1..];

        // Extract msg=audit(timestamp:serial):
        let rest = rest.strip_prefix("msg=audit(")?;
        let paren_close = rest.find(')')?;
        let ts_serial = &rest[..paren_close];
        let colon_idx = ts_serial.find(':')?;
        let timestamp: f64 = ts_serial[..colon_idx].parse().ok()?;
        let serial: u64 = ts_serial[colon_idx + 1..].parse().ok()?;

        // After "): " comes the field=value pairs
        let rest = &rest[paren_close..];
        let rest = rest.strip_prefix("):").unwrap_or(rest.strip_prefix(")").unwrap_or(""));
        let rest = rest.trim_start();

        let mut fields = HashMap::new();
        parse_field_pairs(rest, &mut fields);

        Some(Self {
            msg_type,
            timestamp,
            serial,
            fields,
            raw_line: line.to_string(),
        })
    }

    fn get_field(&self, name: &str) -> Option<&str> {
        self.fields.get(name).map(|s| s.as_str())
    }

    fn success(&self) -> Option<bool> {
        match self.get_field("success") {
            Some("yes") => Some(true),
            Some("no") => Some(false),
            _ => None,
        }
    }

    fn syscall_name(&self) -> Option<&str> {
        self.get_field("syscall")
    }

    fn key(&self) -> Option<&str> {
        self.get_field("key").map(|k| k.trim_matches('"'))
    }

    fn auid(&self) -> Option<&str> {
        self.get_field("auid")
    }

    fn uid(&self) -> Option<&str> {
        self.get_field("uid")
    }

    fn exe(&self) -> Option<&str> {
        self.get_field("exe").map(|e| e.trim_matches('"'))
    }

    fn _comm(&self) -> Option<&str> {
        self.get_field("comm").map(|c| c.trim_matches('"'))
    }

    fn name_field(&self) -> Option<&str> {
        self.get_field("name").map(|n| n.trim_matches('"'))
    }

    fn acct(&self) -> Option<&str> {
        self.get_field("acct").map(|a| a.trim_matches('"'))
    }

    fn terminal(&self) -> Option<&str> {
        self.get_field("terminal")
    }

    fn res(&self) -> Option<&str> {
        self.get_field("res")
    }

    fn pid(&self) -> Option<&str> {
        self.get_field("pid")
    }

    fn format_timestamp(&self) -> String {
        // Format as a human-readable date from the epoch timestamp.
        // Simple approximation: seconds since epoch -> date string.
        let secs = self.timestamp as u64;
        format_epoch_secs(secs)
    }
}

/// Parse space-separated field=value pairs from audit log line.
fn parse_field_pairs(s: &str, fields: &mut HashMap<String, String>) {
    let mut rest = s;
    while !rest.is_empty() {
        // Find the next '='
        let eq_idx = match rest.find('=') {
            Some(i) => i,
            None => break,
        };
        let key = rest[..eq_idx].trim().to_string();
        rest = &rest[eq_idx + 1..];

        // Value may be quoted
        if rest.starts_with('"') {
            // Find closing quote
            let end_quote = rest[1..].find('"').map(|i| i + 1);
            if let Some(eq) = end_quote {
                let value = rest[1..eq].to_string();
                fields.insert(key, value);
                rest = &rest[eq + 1..];
                rest = rest.trim_start();
            } else {
                // Unterminated quote, take rest
                fields.insert(key, rest[1..].to_string());
                break;
            }
        } else {
            // Unquoted: value goes until next space
            let end = rest.find(' ').unwrap_or(rest.len());
            let value = rest[..end].to_string();
            fields.insert(key, value);
            rest = if end < rest.len() {
                &rest[end + 1..]
            } else {
                ""
            };
        }
    }
}

/// Simple epoch seconds to date string.
fn format_epoch_secs(secs: u64) -> String {
    // Simplified date formatting without external crate.
    // Compute year/month/day/hour/min/sec from Unix epoch.
    let days_since_epoch = secs / 86400;
    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    let (year, month, day) = days_to_ymd(days_since_epoch);

    format!(
        "{:04}/{:02}/{:02} {:02}:{:02}:{:02}",
        year, month, day, hours, minutes, seconds
    )
}

/// Convert days since Unix epoch to (year, month, day).
fn days_to_ymd(days: u64) -> (u64, u64, u64) {
    // Algorithm based on Howard Hinnant's civil_from_days.
    let z = days as i64 + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y as u64, m, d)
}

/// Parse a simple time string into epoch seconds.
/// Accepts formats: epoch seconds, "YYYY/MM/DD HH:MM:SS", "YYYY-MM-DD HH:MM:SS",
/// "today", "now", "yesterday", "this-week", "this-month".
fn parse_time_spec(s: &str) -> Option<f64> {
    match s {
        "now" => Some(simulated_now()),
        "today" => {
            let now = simulated_now() as u64;
            let day_start = (now / 86400) * 86400;
            Some(day_start as f64)
        }
        "yesterday" => {
            let now = simulated_now() as u64;
            let day_start = (now / 86400) * 86400;
            Some((day_start - 86400) as f64)
        }
        "this-week" => {
            let now = simulated_now() as u64;
            let day_start = (now / 86400) * 86400;
            // Approximate: go back up to 7 days
            Some((day_start - 7 * 86400) as f64)
        }
        "this-month" => {
            let now = simulated_now() as u64;
            let day_start = (now / 86400) * 86400;
            Some((day_start - 30 * 86400) as f64)
        }
        _ => {
            // Try parsing as epoch seconds
            if let Ok(v) = s.parse::<f64>() {
                return Some(v);
            }
            // Try YYYY/MM/DD HH:MM:SS or YYYY-MM-DD HH:MM:SS
            let s = s.replace('-', "/");
            let parts: Vec<&str> = s.split(' ').collect();
            if parts.is_empty() {
                return None;
            }
            let date_parts: Vec<&str> = parts[0].split('/').collect();
            if date_parts.len() != 3 {
                return None;
            }
            let year: u64 = date_parts[0].parse().ok()?;
            let month: u64 = date_parts[1].parse().ok()?;
            let day: u64 = date_parts[2].parse().ok()?;

            let mut hours: u64 = 0;
            let mut minutes: u64 = 0;
            let mut secs: u64 = 0;
            if parts.len() >= 2 {
                let time_parts: Vec<&str> = parts[1].split(':').collect();
                if !time_parts.is_empty() {
                    hours = time_parts[0].parse().ok()?;
                }
                if time_parts.len() >= 2 {
                    minutes = time_parts[1].parse().ok()?;
                }
                if time_parts.len() >= 3 {
                    secs = time_parts[2].parse().ok()?;
                }
            }

            let days = ymd_to_days(year, month, day)?;
            Some((days * 86400 + hours * 3600 + minutes * 60 + secs) as f64)
        }
    }
}

/// Convert (year, month, day) to days since Unix epoch.
fn ymd_to_days(year: u64, month: u64, day: u64) -> Option<u64> {
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    // Howard Hinnant's days_from_civil (inverse of days_to_ymd).
    let y = if month <= 2 { year as i64 - 1 } else { year as i64 };
    let m = if month <= 2 { month + 9 } else { month - 3 };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u64;
    let doy = (153 * m + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let result = era * 146097 + doe as i64 - 719468;
    if result < 0 {
        None
    } else {
        Some(result as u64)
    }
}

/// Return simulated "now" timestamp (epoch seconds).
fn simulated_now() -> f64 {
    // In a real OS this would use the system clock.
    // For simulation, use a fixed reference or env var.
    if let Ok(v) = env::var("AUDIT_SIMULATED_NOW")
        && let Ok(t) = v.parse::<f64>() {
            return t;
        }
    // Default: 2025-01-01 00:00:00 UTC = 1735689600
    1_735_689_600.0
}

// ============================================================================
// Audit rule store
// ============================================================================

/// In-memory audit rule store. In a real system, rules would be managed
/// via netlink to the kernel audit subsystem.
#[derive(Clone, Debug)]
struct RuleStore {
    rules: Vec<AuditRule>,
    status: AuditStatus,
}

impl RuleStore {
    fn new() -> Self {
        Self {
            rules: Vec::new(),
            status: AuditStatus::new(),
        }
    }

    fn add_rule(&mut self, rule: AuditRule) {
        self.rules.push(rule);
    }

    fn delete_rule(&mut self, action: RuleAction, filter: RuleFilter, syscall: Option<&str>) -> bool {
        let initial_len = self.rules.len();
        self.rules.retain(|r| {
            if let AuditRule::Syscall {
                action: a,
                filter: f,
                syscall: sc,
                ..
            } = r
                && *a == action && *f == filter {
                    if let Some(target_sc) = syscall
                        && let Some(rule_sc) = sc {
                            return rule_sc != target_sc;
                        }
                    return false;
                }
            true
        });
        self.rules.len() < initial_len
    }

    fn delete_all(&mut self) -> usize {
        let count = self.rules.len();
        self.rules.clear();
        count
    }

    fn list_rules(&self) -> &[AuditRule] {
        &self.rules
    }
}

// ============================================================================
// auditd configuration
// ============================================================================

#[derive(Clone, Debug)]
struct AuditdConfig {
    log_file: String,
    log_format: String,
    max_log_file: u64,
    max_log_file_action: String,
    num_logs: u32,
    name_format: String,
    name: String,
    space_left: u64,
    space_left_action: String,
    admin_space_left: u64,
    admin_space_left_action: String,
    disk_full_action: String,
    disk_error_action: String,
    flush: String,
    freq: u32,
    priority_boost: u32,
    disp_qos: String,
    dispatcher: String,
    tcp_listen_port: u32,
    tcp_max_per_addr: u32,
    write_logs: bool,
}

impl AuditdConfig {
    fn new() -> Self {
        Self {
            log_file: DEFAULT_LOG_FILE.to_string(),
            log_format: "ENRICHED".to_string(),
            max_log_file: 8,
            max_log_file_action: "ROTATE".to_string(),
            num_logs: 5,
            name_format: "NONE".to_string(),
            name: String::new(),
            space_left: 75,
            space_left_action: "SYSLOG".to_string(),
            admin_space_left: 50,
            admin_space_left_action: "SUSPEND".to_string(),
            disk_full_action: "SUSPEND".to_string(),
            disk_error_action: "SUSPEND".to_string(),
            flush: "INCREMENTAL_ASYNC".to_string(),
            freq: 50,
            priority_boost: 4,
            disp_qos: "lossy".to_string(),
            dispatcher: "/sbin/audispd".to_string(),
            tcp_listen_port: 0,
            tcp_max_per_addr: 1,
            write_logs: true,
        }
    }

    fn parse_config_file(path: &str) -> Self {
        let mut config = Self::new();

        let content = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => return config,
        };

        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = line.split_once('=') {
                let key = key.trim();
                let value = value.trim();
                config.apply_setting(key, value);
            }
        }

        config
    }

    fn apply_setting(&mut self, key: &str, value: &str) {
        match key {
            "log_file" => self.log_file = value.to_string(),
            "log_format" => self.log_format = value.to_string(),
            "max_log_file" => {
                if let Ok(v) = value.parse() {
                    self.max_log_file = v;
                }
            }
            "max_log_file_action" => self.max_log_file_action = value.to_string(),
            "num_logs" => {
                if let Ok(v) = value.parse() {
                    self.num_logs = v;
                }
            }
            "name_format" => self.name_format = value.to_string(),
            "name" => self.name = value.to_string(),
            "space_left" => {
                if let Ok(v) = value.parse() {
                    self.space_left = v;
                }
            }
            "space_left_action" => self.space_left_action = value.to_string(),
            "admin_space_left" => {
                if let Ok(v) = value.parse() {
                    self.admin_space_left = v;
                }
            }
            "admin_space_left_action" => self.admin_space_left_action = value.to_string(),
            "disk_full_action" => self.disk_full_action = value.to_string(),
            "disk_error_action" => self.disk_error_action = value.to_string(),
            "flush" => self.flush = value.to_string(),
            "freq" => {
                if let Ok(v) = value.parse() {
                    self.freq = v;
                }
            }
            "priority_boost" => {
                if let Ok(v) = value.parse() {
                    self.priority_boost = v;
                }
            }
            "disp_qos" => self.disp_qos = value.to_string(),
            "dispatcher" => self.dispatcher = value.to_string(),
            "tcp_listen_port" => {
                if let Ok(v) = value.parse() {
                    self.tcp_listen_port = v;
                }
            }
            "tcp_max_per_addr" => {
                if let Ok(v) = value.parse() {
                    self.tcp_max_per_addr = v;
                }
            }
            "write_logs" => self.write_logs = value == "yes",
            _ => {}
        }
    }
}

impl fmt::Display for AuditdConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "log_file = {}", self.log_file)?;
        writeln!(f, "log_format = {}", self.log_format)?;
        writeln!(f, "max_log_file = {}", self.max_log_file)?;
        writeln!(f, "max_log_file_action = {}", self.max_log_file_action)?;
        writeln!(f, "num_logs = {}", self.num_logs)?;
        writeln!(f, "name_format = {}", self.name_format)?;
        writeln!(f, "space_left = {}", self.space_left)?;
        writeln!(f, "space_left_action = {}", self.space_left_action)?;
        writeln!(f, "admin_space_left = {}", self.admin_space_left)?;
        writeln!(f, "admin_space_left_action = {}", self.admin_space_left_action)?;
        writeln!(f, "disk_full_action = {}", self.disk_full_action)?;
        writeln!(f, "disk_error_action = {}", self.disk_error_action)?;
        writeln!(f, "flush = {}", self.flush)?;
        writeln!(f, "freq = {}", self.freq)?;
        writeln!(f, "priority_boost = {}", self.priority_boost)?;
        writeln!(f, "disp_qos = {}", self.disp_qos)?;
        writeln!(f, "dispatcher = {}", self.dispatcher)?;
        writeln!(f, "write_logs = {}", if self.write_logs { "yes" } else { "no" })
    }
}

// ============================================================================
// Log loading
// ============================================================================

fn load_audit_log(path: &str) -> Vec<AuditRecord> {
    let mut records = Vec::new();
    let file = match fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return records,
    };
    let reader = io::BufReader::new(file);
    for line in reader.lines() {
        if let Ok(line) = line
            && let Some(record) = AuditRecord::parse(&line) {
                records.push(record);
            }
    }
    records
}

#[cfg(test)]
fn load_audit_log_from_str(content: &str) -> Vec<AuditRecord> {
    let mut records = Vec::new();
    for line in content.lines() {
        if let Some(record) = AuditRecord::parse(line) {
            records.push(record);
        }
    }
    records
}

// ============================================================================
// ausearch — search audit logs
// ============================================================================

#[derive(Clone, Debug, Default)]
struct SearchCriteria {
    key: Option<String>,
    syscall: Option<String>,
    success: Option<bool>,
    user: Option<String>,
    start_time: Option<f64>,
    end_time: Option<f64>,
    message_type: Option<MessageType>,
    interpret: bool,
    format: OutputFormat,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
enum OutputFormat {
    Raw,
    #[default]
    Text,
    Csv,
}

impl OutputFormat {
    fn from_str(s: &str) -> Option<Self> {
        match s {
            "raw" => Some(Self::Raw),
            "text" => Some(Self::Text),
            "csv" => Some(Self::Csv),
            _ => None,
        }
    }
}

fn record_matches(record: &AuditRecord, criteria: &SearchCriteria) -> bool {
    if let Some(ref key) = criteria.key {
        match record.key() {
            Some(k) if k == key => {}
            _ => return false,
        }
    }
    if let Some(ref syscall) = criteria.syscall {
        match record.syscall_name() {
            Some(sc) if sc == syscall => {}
            _ => return false,
        }
    }
    if let Some(success) = criteria.success {
        match record.success() {
            Some(s) if s == success => {}
            Some(_) => return false,
            None => return false, // Records without success field do not match
        }
    }
    if let Some(ref user) = criteria.user {
        let matches_auid = record.auid().map(|a| a == user).unwrap_or(false);
        let matches_uid = record.uid().map(|u| u == user).unwrap_or(false);
        let matches_acct = record.acct().map(|a| a == user).unwrap_or(false);
        if !matches_auid && !matches_uid && !matches_acct {
            return false;
        }
    }
    if let Some(start) = criteria.start_time
        && record.timestamp < start {
            return false;
        }
    if let Some(end) = criteria.end_time
        && record.timestamp > end {
            return false;
        }
    if let Some(ref mtype) = criteria.message_type
        && record.msg_type != *mtype {
            return false;
        }
    true
}

fn format_record(record: &AuditRecord, format: &OutputFormat, _interpret: bool) -> String {
    match format {
        OutputFormat::Raw => record.raw_line.clone(),
        OutputFormat::Text => {
            let mut s = String::new();
            s.push_str(&format!("----\ntime->{} : serial={}\n", record.format_timestamp(), record.serial));
            s.push_str(&format!("type={}", record.msg_type));
            for (k, v) in &record.fields {
                s.push_str(&format!(" {}={}", k, v));
            }
            s.push('\n');
            s
        }
        OutputFormat::Csv => {
            let mut parts = vec![
                record.msg_type.to_string(),
                format!("{:.3}", record.timestamp),
                record.serial.to_string(),
            ];
            // Add common fields
            for field_name in &["pid", "uid", "auid", "syscall", "success", "exe", "key"] {
                parts.push(record.get_field(field_name).unwrap_or("").to_string());
            }
            parts.join(",")
        }
    }
}

fn run_ausearch(args: &[String]) -> i32 {
    let mut criteria = SearchCriteria::default();
    let mut log_file = DEFAULT_LOG_FILE.to_string();
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "-k" | "--key" => {
                i += 1;
                if i < args.len() {
                    criteria.key = Some(args[i].clone());
                } else {
                    eprintln!("ausearch: -k requires an argument");
                    return 1;
                }
            }
            "-sc" | "--syscall" => {
                i += 1;
                if i < args.len() {
                    criteria.syscall = Some(args[i].clone());
                } else {
                    eprintln!("ausearch: -sc requires an argument");
                    return 1;
                }
            }
            "-sv" | "--success" => {
                i += 1;
                if i < args.len() {
                    match args[i].as_str() {
                        "yes" | "success" => criteria.success = Some(true),
                        "no" | "fail" | "failed" => criteria.success = Some(false),
                        _ => {
                            eprintln!("ausearch: invalid success value: {}", args[i]);
                            return 1;
                        }
                    }
                } else {
                    eprintln!("ausearch: -sv requires an argument");
                    return 1;
                }
            }
            "-ua" | "--uid-all" => {
                i += 1;
                if i < args.len() {
                    criteria.user = Some(args[i].clone());
                } else {
                    eprintln!("ausearch: -ua requires an argument");
                    return 1;
                }
            }
            "-ts" | "--start" => {
                i += 1;
                if i < args.len() {
                    // Check if next arg is also part of time (e.g., "2025/01/01 12:00:00")
                    let mut time_str = args[i].clone();
                    if i + 1 < args.len() && args[i + 1].contains(':') && !args[i + 1].starts_with('-') {
                        time_str.push(' ');
                        time_str.push_str(&args[i + 1]);
                        i += 1;
                    }
                    match parse_time_spec(&time_str) {
                        Some(t) => criteria.start_time = Some(t),
                        None => {
                            eprintln!("ausearch: invalid start time: {}", time_str);
                            return 1;
                        }
                    }
                } else {
                    eprintln!("ausearch: -ts requires an argument");
                    return 1;
                }
            }
            "-te" | "--end" => {
                i += 1;
                if i < args.len() {
                    let mut time_str = args[i].clone();
                    if i + 1 < args.len() && args[i + 1].contains(':') && !args[i + 1].starts_with('-') {
                        time_str.push(' ');
                        time_str.push_str(&args[i + 1]);
                        i += 1;
                    }
                    match parse_time_spec(&time_str) {
                        Some(t) => criteria.end_time = Some(t),
                        None => {
                            eprintln!("ausearch: invalid end time: {}", time_str);
                            return 1;
                        }
                    }
                } else {
                    eprintln!("ausearch: -te requires an argument");
                    return 1;
                }
            }
            "-m" | "--message" => {
                i += 1;
                if i < args.len() {
                    criteria.message_type = Some(MessageType::from_str(&args[i]));
                } else {
                    eprintln!("ausearch: -m requires an argument");
                    return 1;
                }
            }
            "-i" | "--interpret" => {
                criteria.interpret = true;
            }
            "--format" => {
                i += 1;
                if i < args.len() {
                    match OutputFormat::from_str(&args[i]) {
                        Some(f) => criteria.format = f,
                        None => {
                            eprintln!("ausearch: invalid format: {}", args[i]);
                            return 1;
                        }
                    }
                } else {
                    eprintln!("ausearch: --format requires an argument");
                    return 1;
                }
            }
            "-if" | "--input" => {
                i += 1;
                if i < args.len() {
                    log_file = args[i].clone();
                } else {
                    eprintln!("ausearch: -if requires an argument");
                    return 1;
                }
            }
            "-h" | "--help" => {
                print_ausearch_help();
                return 0;
            }
            "-v" | "--version" => {
                println!("ausearch {VERSION}");
                return 0;
            }
            other => {
                eprintln!("ausearch: unknown option: {other}");
                return 1;
            }
        }
        i += 1;
    }

    let records = load_audit_log(&log_file);
    if records.is_empty() {
        eprintln!("<no matches>");
        return 1;
    }

    let mut found = false;
    if criteria.format == OutputFormat::Csv {
        println!("type,timestamp,serial,pid,uid,auid,syscall,success,exe,key");
    }
    for record in &records {
        if record_matches(record, &criteria) {
            println!("{}", format_record(record, &criteria.format, criteria.interpret));
            found = true;
        }
    }

    if !found {
        eprintln!("<no matches>");
        return 1;
    }

    0
}

fn print_ausearch_help() {
    println!("Usage: ausearch [options]");
    println!();
    println!("Options:");
    println!("  -k, --key KEY          Search by key");
    println!("  -sc, --syscall SYSCALL Search by syscall");
    println!("  -sv, --success yes|no  Search by success/fail");
    println!("  -ua, --uid-all USER    Search by audit user");
    println!("  -ts, --start TIME      Start time for search");
    println!("  -te, --end TIME        End time for search");
    println!("  -m, --message TYPE     Search by message type");
    println!("  -i, --interpret        Interpret numeric fields");
    println!("  --format raw|text|csv  Output format (default: text)");
    println!("  -if, --input FILE      Input log file");
    println!("  -h, --help             Show this help");
    println!("  -v, --version          Show version");
    println!();
    println!("Time formats: epoch, YYYY/MM/DD HH:MM:SS, today, now, yesterday");
}

// ============================================================================
// aureport — audit report generator
// ============================================================================

#[derive(Clone, Debug, Default)]
struct ReportOptions {
    auth: bool,
    login: bool,
    file: bool,
    syscall: bool,
    summary: bool,
    failed: bool,
    start_time: Option<f64>,
    end_time: Option<f64>,
    log_file: String,
}

fn run_aureport(args: &[String]) -> i32 {
    let mut opts = ReportOptions {
        log_file: DEFAULT_LOG_FILE.to_string(),
        ..Default::default()
    };
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "--auth" | "-au" => opts.auth = true,
            "--login" | "-l" => opts.login = true,
            "--file" | "-f" => opts.file = true,
            "--syscall" | "-s" => opts.syscall = true,
            "--summary" => opts.summary = true,
            "--failed" => opts.failed = true,
            "-ts" | "--start" => {
                i += 1;
                if i < args.len() {
                    let mut time_str = args[i].clone();
                    if i + 1 < args.len() && args[i + 1].contains(':') && !args[i + 1].starts_with('-') {
                        time_str.push(' ');
                        time_str.push_str(&args[i + 1]);
                        i += 1;
                    }
                    match parse_time_spec(&time_str) {
                        Some(t) => opts.start_time = Some(t),
                        None => {
                            eprintln!("aureport: invalid start time: {}", time_str);
                            return 1;
                        }
                    }
                } else {
                    eprintln!("aureport: -ts requires an argument");
                    return 1;
                }
            }
            "-te" | "--end" => {
                i += 1;
                if i < args.len() {
                    let mut time_str = args[i].clone();
                    if i + 1 < args.len() && args[i + 1].contains(':') && !args[i + 1].starts_with('-') {
                        time_str.push(' ');
                        time_str.push_str(&args[i + 1]);
                        i += 1;
                    }
                    match parse_time_spec(&time_str) {
                        Some(t) => opts.end_time = Some(t),
                        None => {
                            eprintln!("aureport: invalid end time: {}", time_str);
                            return 1;
                        }
                    }
                } else {
                    eprintln!("aureport: -te requires an argument");
                    return 1;
                }
            }
            "-if" | "--input" => {
                i += 1;
                if i < args.len() {
                    opts.log_file = args[i].clone();
                } else {
                    eprintln!("aureport: -if requires an argument");
                    return 1;
                }
            }
            "-h" | "--help" => {
                print_aureport_help();
                return 0;
            }
            "-v" | "--version" => {
                println!("aureport {VERSION}");
                return 0;
            }
            other => {
                eprintln!("aureport: unknown option: {other}");
                return 1;
            }
        }
        i += 1;
    }

    let records = load_audit_log(&opts.log_file);
    let records = filter_by_time(&records, opts.start_time, opts.end_time);

    if !opts.auth && !opts.login && !opts.file && !opts.syscall {
        // Default: summary overview
        report_summary_overview(&records, opts.failed);
    } else {
        if opts.auth {
            report_auth(&records, opts.summary, opts.failed);
        }
        if opts.login {
            report_login(&records, opts.summary, opts.failed);
        }
        if opts.file {
            report_file(&records, opts.summary, opts.failed);
        }
        if opts.syscall {
            report_syscall(&records, opts.summary, opts.failed);
        }
    }

    0
}

fn filter_by_time(records: &[AuditRecord], start: Option<f64>, end: Option<f64>) -> Vec<&AuditRecord> {
    records
        .iter()
        .filter(|r| {
            if let Some(s) = start
                && r.timestamp < s {
                    return false;
                }
            if let Some(e) = end
                && r.timestamp > e {
                    return false;
                }
            true
        })
        .collect()
}

fn report_summary_overview(records: &[&AuditRecord], failed_only: bool) {
    println!("Summary Report");
    println!("======================");

    let mut type_counts: HashMap<MessageType, usize> = HashMap::new();
    let mut total = 0usize;
    let mut failed = 0usize;

    for record in records {
        if failed_only {
            if let Some(false) = record.success() {
                // pass
            } else if record.res() == Some("failed") || record.res() == Some("no") {
                // pass
            } else {
                continue;
            }
        }

        *type_counts.entry(record.msg_type).or_insert(0) += 1;
        total += 1;
        if record.success() == Some(false) || record.res() == Some("failed") {
            failed += 1;
        }
    }

    println!("Total events: {total}");
    println!("Failed events: {failed}");
    println!();
    println!("Events by type:");

    let mut sorted: Vec<_> = type_counts.into_iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(&a.1));
    for (mtype, count) in &sorted {
        println!("  {:<20} {}", mtype.as_str(), count);
    }
}

fn report_auth(records: &[&AuditRecord], summary: bool, failed_only: bool) {
    println!("Authentication Report");
    println!("======================");

    let filtered: Vec<&&AuditRecord> = records
        .iter()
        .filter(|r| r.msg_type.is_auth_related())
        .filter(|r| {
            if failed_only {
                r.res() == Some("failed") || r.res() == Some("no") || r.success() == Some(false)
            } else {
                true
            }
        })
        .collect();

    if summary {
        let mut user_counts: HashMap<String, usize> = HashMap::new();
        for record in &filtered {
            let user = record.acct().unwrap_or("unknown").to_string();
            *user_counts.entry(user).or_insert(0) += 1;
        }
        println!("Number  Account");
        println!("======  =======");
        let mut sorted: Vec<_> = user_counts.into_iter().collect();
        sorted.sort_by(|a, b| b.1.cmp(&a.1));
        for (user, count) in &sorted {
            println!("{:<8}{}", count, user);
        }
    } else {
        println!("{:<5} {:<20} {:<12} {:<12} {:<10} {:<8}", "#", "Date", "Account", "Host", "Terminal", "Result");
        println!("{}", "=".repeat(70));
        for (idx, record) in filtered.iter().enumerate() {
            let date = record.format_timestamp();
            let acct = record.acct().unwrap_or("?");
            let host = record.get_field("hostname").unwrap_or("?");
            let term = record.terminal().unwrap_or("?");
            let res = record.res().unwrap_or("?");
            println!("{:<5} {:<20} {:<12} {:<12} {:<10} {:<8}", idx + 1, date, acct, host, term, res);
        }
    }
    println!();
}

fn report_login(records: &[&AuditRecord], summary: bool, failed_only: bool) {
    println!("Login Report");
    println!("======================");

    let filtered: Vec<&&AuditRecord> = records
        .iter()
        .filter(|r| r.msg_type.is_login_related())
        .filter(|r| {
            if failed_only {
                r.res() == Some("failed") || r.res() == Some("no") || r.success() == Some(false)
            } else {
                true
            }
        })
        .collect();

    if summary {
        let mut user_counts: HashMap<String, usize> = HashMap::new();
        for record in &filtered {
            let user = record.acct().or_else(|| record.auid()).unwrap_or("unknown").to_string();
            *user_counts.entry(user).or_insert(0) += 1;
        }
        println!("Number  Account");
        println!("======  =======");
        let mut sorted: Vec<_> = user_counts.into_iter().collect();
        sorted.sort_by(|a, b| b.1.cmp(&a.1));
        for (user, count) in &sorted {
            println!("{:<8}{}", count, user);
        }
    } else {
        println!("{:<5} {:<20} {:<12} {:<12} {:<10} {:<8}", "#", "Date", "Account", "Host", "Terminal", "Result");
        println!("{}", "=".repeat(70));
        for (idx, record) in filtered.iter().enumerate() {
            let date = record.format_timestamp();
            let acct = record.acct().or_else(|| record.auid()).unwrap_or("?");
            let host = record.get_field("hostname").unwrap_or("?");
            let term = record.terminal().unwrap_or("?");
            let res = record.res().unwrap_or("?");
            println!("{:<5} {:<20} {:<12} {:<12} {:<10} {:<8}", idx + 1, date, acct, host, term, res);
        }
    }
    println!();
}

fn report_file(records: &[&AuditRecord], summary: bool, failed_only: bool) {
    println!("File Report");
    println!("======================");

    let filtered: Vec<&&AuditRecord> = records
        .iter()
        .filter(|r| r.msg_type == MessageType::Path || r.name_field().is_some())
        .filter(|r| {
            if failed_only {
                r.success() == Some(false) || r.res() == Some("failed")
            } else {
                true
            }
        })
        .collect();

    if summary {
        let mut file_counts: HashMap<String, usize> = HashMap::new();
        for record in &filtered {
            let name = record.name_field().unwrap_or("unknown").to_string();
            *file_counts.entry(name).or_insert(0) += 1;
        }
        println!("Number  File");
        println!("======  ====");
        let mut sorted: Vec<_> = file_counts.into_iter().collect();
        sorted.sort_by(|a, b| b.1.cmp(&a.1));
        for (name, count) in &sorted {
            println!("{:<8}{}", count, name);
        }
    } else {
        println!("{:<5} {:<20} {:<30} {:<10} {:<10}", "#", "Date", "File", "Syscall", "Result");
        println!("{}", "=".repeat(78));
        for (idx, record) in filtered.iter().enumerate() {
            let date = record.format_timestamp();
            let name = record.name_field().unwrap_or("?");
            let sc = record.syscall_name().unwrap_or("?");
            let success = match record.success() {
                Some(true) => "yes",
                Some(false) => "no",
                None => "?",
            };
            println!("{:<5} {:<20} {:<30} {:<10} {:<10}", idx + 1, date, name, sc, success);
        }
    }
    println!();
}

fn report_syscall(records: &[&AuditRecord], summary: bool, failed_only: bool) {
    println!("Syscall Report");
    println!("======================");

    let filtered: Vec<&&AuditRecord> = records
        .iter()
        .filter(|r| r.msg_type == MessageType::Syscall)
        .filter(|r| {
            if failed_only {
                r.success() == Some(false)
            } else {
                true
            }
        })
        .collect();

    if summary {
        let mut sc_counts: HashMap<String, usize> = HashMap::new();
        for record in &filtered {
            let sc = record.syscall_name().unwrap_or("unknown").to_string();
            *sc_counts.entry(sc).or_insert(0) += 1;
        }
        println!("Number  Syscall");
        println!("======  =======");
        let mut sorted: Vec<_> = sc_counts.into_iter().collect();
        sorted.sort_by(|a, b| b.1.cmp(&a.1));
        for (sc, count) in &sorted {
            println!("{:<8}{}", count, sc);
        }
    } else {
        println!("{:<5} {:<20} {:<12} {:<10} {:<10} {:<10}", "#", "Date", "Syscall", "PID", "Exe", "Result");
        println!("{}", "=".repeat(70));
        for (idx, record) in filtered.iter().enumerate() {
            let date = record.format_timestamp();
            let sc = record.syscall_name().unwrap_or("?");
            let pid = record.pid().unwrap_or("?");
            let exe = record.exe().unwrap_or("?");
            let success = match record.success() {
                Some(true) => "yes",
                Some(false) => "no",
                None => "?",
            };
            println!("{:<5} {:<20} {:<12} {:<10} {:<10} {:<10}", idx + 1, date, sc, pid, exe, success);
        }
    }
    println!();
}

fn print_aureport_help() {
    println!("Usage: aureport [options]");
    println!();
    println!("Options:");
    println!("  --auth, -au        Authentication report");
    println!("  --login, -l        Login report");
    println!("  --file, -f         File access report");
    println!("  --syscall, -s      Syscall report");
    println!("  --summary          Summary counts");
    println!("  --failed           Only failures");
    println!("  -ts, --start TIME  Start time for report");
    println!("  -te, --end TIME    End time for report");
    println!("  -if, --input FILE  Input log file");
    println!("  -h, --help         Show this help");
    println!("  -v, --version      Show version");
}

// ============================================================================
// auditctl — audit rule management
// ============================================================================

fn run_auditctl(args: &[String]) -> i32 {
    let mut store = RuleStore::new();

    // Try to load existing rules from a state file (simulated).
    load_rule_store(&mut store);

    let result = process_auditctl_args(args, &mut store);

    // Save the state after modifications.
    save_rule_store(&store);

    result
}

fn process_auditctl_args(args: &[String], store: &mut RuleStore) -> i32 {
    if args.is_empty() {
        print_auditctl_help();
        return 0;
    }

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-w" => {
                // File watch rule: -w path [-p perms] [-k key]
                i += 1;
                if i >= args.len() {
                    eprintln!("auditctl: -w requires a path argument");
                    return 1;
                }
                let path = args[i].clone();
                let mut perms = WatchPerms {
                    read: true,
                    write: true,
                    execute: true,
                    attribute: true,
                };
                let mut key = None;

                // Look ahead for -p and -k
                while i + 1 < args.len() {
                    match args[i + 1].as_str() {
                        "-p" => {
                            i += 2;
                            if i >= args.len() {
                                eprintln!("auditctl: -p requires a permissions argument");
                                return 1;
                            }
                            match WatchPerms::from_str(&args[i]) {
                                Some(p) => perms = p,
                                None => {
                                    eprintln!("auditctl: invalid permissions: {}", args[i]);
                                    return 1;
                                }
                            }
                        }
                        "-k" => {
                            i += 2;
                            if i >= args.len() {
                                eprintln!("auditctl: -k requires a key argument");
                                return 1;
                            }
                            key = Some(args[i].clone());
                        }
                        _ => break,
                    }
                }

                store.add_rule(AuditRule::Watch { path, perms, key });
                println!("Rule added successfully");
            }
            "-a" => {
                // Add syscall rule: -a action,filter [-S syscall] [-F field=value] [-k key]
                i += 1;
                if i >= args.len() {
                    eprintln!("auditctl: -a requires action,filter argument");
                    return 1;
                }
                let action_filter = &args[i];
                let parts: Vec<&str> = action_filter.splitn(2, ',').collect();
                if parts.len() != 2 {
                    eprintln!("auditctl: invalid action,filter: {action_filter}");
                    return 1;
                }
                let action = match RuleAction::from_str(parts[0]) {
                    Some(a) => a,
                    None => {
                        eprintln!("auditctl: invalid action: {}", parts[0]);
                        return 1;
                    }
                };
                let filter = match RuleFilter::from_str(parts[1]) {
                    Some(f) => f,
                    None => {
                        eprintln!("auditctl: invalid filter: {}", parts[1]);
                        return 1;
                    }
                };

                let mut syscall = None;
                let mut fields = Vec::new();
                let mut key = None;

                while i + 1 < args.len() {
                    match args[i + 1].as_str() {
                        "-S" => {
                            i += 2;
                            if i >= args.len() {
                                eprintln!("auditctl: -S requires a syscall argument");
                                return 1;
                            }
                            syscall = Some(args[i].clone());
                        }
                        "-F" => {
                            i += 2;
                            if i >= args.len() {
                                eprintln!("auditctl: -F requires a field=value argument");
                                return 1;
                            }
                            match parse_field_filter(&args[i]) {
                                Some(f) => fields.push(f),
                                None => {
                                    eprintln!("auditctl: invalid field filter: {}", args[i]);
                                    return 1;
                                }
                            }
                        }
                        "-k" => {
                            i += 2;
                            if i >= args.len() {
                                eprintln!("auditctl: -k requires a key argument");
                                return 1;
                            }
                            key = Some(args[i].clone());
                        }
                        _ => break,
                    }
                }

                store.add_rule(AuditRule::Syscall {
                    action,
                    filter,
                    syscall,
                    fields,
                    key,
                });
                println!("Rule added successfully");
            }
            "-d" => {
                // Delete syscall rule: -d action,filter [-S syscall]
                i += 1;
                if i >= args.len() {
                    eprintln!("auditctl: -d requires action,filter argument");
                    return 1;
                }
                let action_filter = &args[i];
                let parts: Vec<&str> = action_filter.splitn(2, ',').collect();
                if parts.len() != 2 {
                    eprintln!("auditctl: invalid action,filter: {action_filter}");
                    return 1;
                }
                let action = match RuleAction::from_str(parts[0]) {
                    Some(a) => a,
                    None => {
                        eprintln!("auditctl: invalid action: {}", parts[0]);
                        return 1;
                    }
                };
                let filter = match RuleFilter::from_str(parts[1]) {
                    Some(f) => f,
                    None => {
                        eprintln!("auditctl: invalid filter: {}", parts[1]);
                        return 1;
                    }
                };

                let mut syscall = None;
                if i + 1 < args.len() && args[i + 1] == "-S" {
                    i += 2;
                    if i >= args.len() {
                        eprintln!("auditctl: -S requires a syscall argument");
                        return 1;
                    }
                    syscall = Some(args[i].as_str());
                }

                if store.delete_rule(action, filter, syscall) {
                    println!("Rule deleted successfully");
                } else {
                    eprintln!("auditctl: rule not found");
                    return 1;
                }
            }
            "-D" => {
                let count = store.delete_all();
                println!("Deleted {count} rules");
            }
            "-l" => {
                let rules = store.list_rules();
                if rules.is_empty() {
                    println!("No rules");
                } else {
                    for rule in rules {
                        println!("{rule}");
                    }
                }
            }
            "-s" => {
                print!("{}", store.status);
            }
            "-e" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("auditctl: -e requires 0, 1, or 2");
                    return 1;
                }
                match args[i].as_str() {
                    "0" => {
                        store.status.enabled = 0;
                        println!("Auditing disabled");
                    }
                    "1" => {
                        store.status.enabled = 1;
                        println!("Auditing enabled");
                    }
                    "2" => {
                        store.status.enabled = 2;
                        println!("Auditing locked (immutable)");
                    }
                    other => {
                        eprintln!("auditctl: invalid enable value: {other}");
                        return 1;
                    }
                }
            }
            "-b" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("auditctl: -b requires a backlog limit value");
                    return 1;
                }
                match args[i].parse::<u32>() {
                    Ok(v) => {
                        store.status.backlog_limit = v;
                        println!("Backlog limit set to {v}");
                    }
                    Err(_) => {
                        eprintln!("auditctl: invalid backlog limit: {}", args[i]);
                        return 1;
                    }
                }
            }
            "-r" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("auditctl: -r requires a rate limit value");
                    return 1;
                }
                match args[i].parse::<u32>() {
                    Ok(v) => {
                        store.status.rate_limit = v;
                        println!("Rate limit set to {v}");
                    }
                    Err(_) => {
                        eprintln!("auditctl: invalid rate limit: {}", args[i]);
                        return 1;
                    }
                }
            }
            "-h" | "--help" => {
                print_auditctl_help();
                return 0;
            }
            "-v" | "--version" => {
                println!("auditctl {VERSION}");
                return 0;
            }
            other => {
                eprintln!("auditctl: unknown option: {other}");
                return 1;
            }
        }
        i += 1;
    }

    0
}

fn parse_field_filter(s: &str) -> Option<FieldFilter> {
    // Support =, !=, <, >, <=, >= operators
    for op in &["!=", "<=", ">=", "=", "<", ">"] {
        if let Some(idx) = s.find(op) {
            let field = s[..idx].to_string();
            let value = s[idx + op.len()..].to_string();
            if field.is_empty() || value.is_empty() {
                return None;
            }
            return Some(FieldFilter {
                field,
                op: (*op).to_string(),
                value,
            });
        }
    }
    None
}

fn print_auditctl_help() {
    println!("Usage: auditctl [options]");
    println!();
    println!("Options:");
    println!("  -w path -p rwxa -k key    Add file watch rule");
    println!("  -a action,filter [-S syscall] [-F field=value] [-k key]");
    println!("                            Add syscall rule");
    println!("  -d action,filter [-S syscall]");
    println!("                            Delete syscall rule");
    println!("  -D                        Delete all rules");
    println!("  -l                        List rules");
    println!("  -s                        Show audit status");
    println!("  -e 0|1|2                  Enable/disable/lock auditing");
    println!("  -b LIMIT                  Set backlog limit");
    println!("  -r LIMIT                  Set rate limit");
    println!("  -h, --help                Show this help");
    println!("  -v, --version             Show version");
    println!();
    println!("Actions: always, never");
    println!("Filters: task, exit, user, exclude");
}

/// Load rule store from a simulated state file.
fn load_rule_store(store: &mut RuleStore) {
    let state_path = audit_state_path();
    let content = match fs::read_to_string(&state_path) {
        Ok(c) => c,
        Err(_) => return,
    };

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(rest) = line.strip_prefix("enabled=") {
            if let Ok(v) = rest.parse() {
                store.status.enabled = v;
            }
        } else if let Some(rest) = line.strip_prefix("rate_limit=") {
            if let Ok(v) = rest.parse() {
                store.status.rate_limit = v;
            }
        } else if let Some(rest) = line.strip_prefix("backlog_limit=") {
            if let Ok(v) = rest.parse() {
                store.status.backlog_limit = v;
            }
        } else if let Some(rest) = line.strip_prefix("rule=") {
            // Parse rule from stored representation
            let rule_args: Vec<String> = rest.split_whitespace().map(|s| s.to_string()).collect();
            let mut temp_store = RuleStore::new();
            process_auditctl_args(&rule_args, &mut temp_store);
            for r in temp_store.rules {
                store.rules.push(r);
            }
        }
    }
}

/// Save rule store to a simulated state file.
fn save_rule_store(store: &RuleStore) {
    let state_path = audit_state_path();
    if let Some(parent) = Path::new(&state_path).parent() {
        let _ = fs::create_dir_all(parent);
    }
    let mut content = String::new();
    content.push_str(&format!("enabled={}\n", store.status.enabled));
    content.push_str(&format!("rate_limit={}\n", store.status.rate_limit));
    content.push_str(&format!("backlog_limit={}\n", store.status.backlog_limit));
    for rule in &store.rules {
        content.push_str(&format!("rule={rule}\n"));
    }
    let _ = fs::write(&state_path, content);
}

fn audit_state_path() -> PathBuf {
    if let Ok(p) = env::var("AUDIT_STATE_FILE") {
        return PathBuf::from(p);
    }
    PathBuf::from("/var/lib/audit/rules.state")
}

// ============================================================================
// auditd — audit daemon
// ============================================================================

fn run_auditd(args: &[String]) -> i32 {
    let mut config_file = DEFAULT_CONFIG_FILE.to_string();
    let mut foreground = false;
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "-c" | "--config" => {
                i += 1;
                if i < args.len() {
                    config_file = args[i].clone();
                } else {
                    eprintln!("auditd: -c requires a config file path");
                    return 1;
                }
            }
            "-f" | "--foreground" => {
                foreground = true;
            }
            "-n" | "--no-fork" => {
                foreground = true;
            }
            "-h" | "--help" => {
                print_auditd_help();
                return 0;
            }
            "-v" | "--version" => {
                println!("auditd {VERSION}");
                return 0;
            }
            other => {
                eprintln!("auditd: unknown option: {other}");
                return 1;
            }
        }
        i += 1;
    }

    let config = AuditdConfig::parse_config_file(&config_file);

    // Create log directory
    if let Some(parent) = Path::new(&config.log_file).parent()
        && let Err(e) = fs::create_dir_all(parent) {
            eprintln!("auditd: cannot create log directory: {e}");
            return 1;
        }

    // Write PID file
    let pid_file = env::var("AUDIT_PID_FILE").unwrap_or_else(|_| DEFAULT_PID_FILE.to_string());
    if let Some(parent) = Path::new(&pid_file).parent() {
        let _ = fs::create_dir_all(parent);
    }
    let pid = process::id();
    if let Err(e) = fs::write(&pid_file, format!("{pid}\n")) {
        eprintln!("auditd: warning: cannot write PID file: {e}");
    }

    println!("auditd: started (pid={pid})");
    println!("auditd: config loaded from {config_file}");
    println!("auditd: logging to {}", config.log_file);
    println!("auditd: max_log_file={}MB, num_logs={}", config.max_log_file, config.num_logs);
    println!("auditd: max_log_file_action={}", config.max_log_file_action);

    if config.write_logs {
        // Ensure log file exists
        let log_path = Path::new(&config.log_file);
        if !log_path.exists()
            && let Err(e) = fs::File::create(log_path) {
                eprintln!("auditd: cannot create log file: {e}");
                return 1;
            }
    }

    if foreground {
        println!("auditd: running in foreground mode");
        // In a real daemon we would enter event loop.
        // For simulation, write a startup event and exit.
        write_audit_event(&config.log_file, MessageType::DaemonStart, &[
            ("op", "start"),
            ("ver", VERSION),
            ("res", "success"),
        ]);
        println!("auditd: daemon event loop would run here");
    } else {
        println!("auditd: would fork to background (simulated)");
        write_audit_event(&config.log_file, MessageType::DaemonStart, &[
            ("op", "start"),
            ("ver", VERSION),
            ("res", "success"),
        ]);
    }

    // Clean up PID file on exit
    let _ = fs::remove_file(&pid_file);

    0
}

fn write_audit_event(log_file: &str, msg_type: MessageType, fields: &[(&str, &str)]) {
    let now = simulated_now();
    let serial = (now * 1000.0) as u64 % 1_000_000;
    let mut line = format!("type={} msg=audit({:.3}:{}):", msg_type, now, serial);
    for (k, v) in fields {
        line.push_str(&format!(" {}={}", k, v));
    }
    line.push('\n');

    if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(log_file) {
        let _ = file.write_all(line.as_bytes());
    }
}

fn print_auditd_help() {
    println!("Usage: auditd [options]");
    println!();
    println!("Options:");
    println!("  -c, --config FILE   Config file path (default: {DEFAULT_CONFIG_FILE})");
    println!("  -f, --foreground    Run in foreground");
    println!("  -n, --no-fork       Don't fork (same as -f)");
    println!("  -h, --help          Show this help");
    println!("  -v, --version       Show version");
}

// ============================================================================
// autrace — trace process using audit
// ============================================================================

fn run_autrace(args: &[String]) -> i32 {
    if args.is_empty() {
        print_autrace_help();
        return 1;
    }

    let mut i = 0;
    let mut _detach = false;

    while i < args.len() {
        match args[i].as_str() {
            "-d" | "--detach" => {
                _detach = true;
                i += 1;
            }
            "-h" | "--help" => {
                print_autrace_help();
                return 0;
            }
            "-v" | "--version" => {
                println!("autrace {VERSION}");
                return 0;
            }
            "-r" => {
                // Delete autrace rules
                println!("autrace: deleting audit rules used by autrace");
                return 0;
            }
            _ => break,
        }
    }

    if i >= args.len() {
        eprintln!("autrace: no program specified");
        return 1;
    }

    let program = &args[i];
    let prog_args = &args[i + 1..];

    println!("autrace: tracing program '{program}'");
    if !prog_args.is_empty() {
        println!("autrace: with arguments: {}", prog_args.join(" "));
    }

    // In a real implementation, we would:
    // 1. Delete existing autrace rules
    // 2. Add syscall audit rules for the process
    // 3. Exec the program
    // 4. Collect audit records
    // 5. Clean up rules

    println!("autrace: adding audit rules for tracing...");
    println!("autrace: rule -a always,exit -F arch=b64 -S all -F pid=<target> -k autrace");
    println!("autrace: executing '{program}'...");
    println!("autrace: program completed");
    println!("autrace: cleaning up audit rules...");
    println!("autrace: trace complete. Use 'ausearch -k autrace' to view results.");

    0
}

fn print_autrace_help() {
    println!("Usage: autrace [-d] program [args...]");
    println!();
    println!("Options:");
    println!("  -d, --detach    Don't wait for program to complete");
    println!("  -r              Delete autrace rules");
    println!("  -h, --help      Show this help");
    println!("  -v, --version   Show version");
    println!();
    println!("Traces the given program by adding audit rules to capture");
    println!("all system calls made by the process.");
}

// ============================================================================
// Main entry point with personality dispatch
// ============================================================================

fn main() {
    let args: Vec<String> = env::args().collect();

    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("auditctl");
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' {
                last_sep = i + 1;
            }
        }
        let base = &s[last_sep..];
        let base = base.strip_suffix(".exe").unwrap_or(base);
        base.to_string()
    };

    let tool_args: Vec<String> = args.into_iter().skip(1).collect();

    let exit_code = match prog_name.as_str() {
        "auditd" => run_auditd(&tool_args),
        "ausearch" => run_ausearch(&tool_args),
        "aureport" => run_aureport(&tool_args),
        "autrace" => run_autrace(&tool_args),
        _ => run_auditctl(&tool_args),
    };

    process::exit(exit_code);
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // MessageType tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_message_type_from_str_syscall() {
        assert_eq!(MessageType::from_str("SYSCALL"), MessageType::Syscall);
    }

    #[test]
    fn test_message_type_from_str_user_auth() {
        assert_eq!(MessageType::from_str("USER_AUTH"), MessageType::UserAuth);
    }

    #[test]
    fn test_message_type_from_str_user_login() {
        assert_eq!(MessageType::from_str("USER_LOGIN"), MessageType::UserLogin);
    }

    #[test]
    fn test_message_type_from_str_path() {
        assert_eq!(MessageType::from_str("PATH"), MessageType::Path);
    }

    #[test]
    fn test_message_type_from_str_cwd() {
        assert_eq!(MessageType::from_str("CWD"), MessageType::Cwd);
    }

    #[test]
    fn test_message_type_from_str_execve() {
        assert_eq!(MessageType::from_str("EXECVE"), MessageType::Execve);
    }

    #[test]
    fn test_message_type_from_str_config() {
        assert_eq!(MessageType::from_str("CONFIG_CHANGE"), MessageType::Config);
    }

    #[test]
    fn test_message_type_from_str_unknown() {
        assert_eq!(MessageType::from_str("GARBAGE"), MessageType::Unknown);
    }

    #[test]
    fn test_message_type_from_str_case_insensitive() {
        assert_eq!(MessageType::from_str("syscall"), MessageType::Syscall);
        assert_eq!(MessageType::from_str("user_auth"), MessageType::UserAuth);
    }

    #[test]
    fn test_message_type_roundtrip() {
        let types = [
            MessageType::Syscall, MessageType::Path, MessageType::Cwd,
            MessageType::Execve, MessageType::UserAuth, MessageType::UserLogin,
            MessageType::DaemonStart, MessageType::DaemonEnd,
        ];
        for t in &types {
            assert_eq!(MessageType::from_str(t.as_str()), *t);
        }
    }

    #[test]
    fn test_message_type_display() {
        assert_eq!(format!("{}", MessageType::Syscall), "SYSCALL");
        assert_eq!(format!("{}", MessageType::UserAuth), "USER_AUTH");
    }

    #[test]
    fn test_message_type_is_auth_related() {
        assert!(MessageType::UserAuth.is_auth_related());
        assert!(MessageType::CredAcq.is_auth_related());
        assert!(!MessageType::Syscall.is_auth_related());
        assert!(!MessageType::Path.is_auth_related());
    }

    #[test]
    fn test_message_type_is_login_related() {
        assert!(MessageType::UserLogin.is_login_related());
        assert!(MessageType::Login.is_login_related());
        assert!(MessageType::UserStart.is_login_related());
        assert!(!MessageType::Syscall.is_login_related());
    }

    #[test]
    fn test_message_type_all_variants() {
        // Verify all string representations parse correctly
        let cases = vec![
            ("PROCTITLE", MessageType::Proctitle),
            ("USER_ACCT", MessageType::UserAcct),
            ("USER_START", MessageType::UserStart),
            ("USER_END", MessageType::UserEnd),
            ("CRED_DISP", MessageType::CredDisp),
            ("CRED_REFR", MessageType::CredRefr),
            ("USER_CMD", MessageType::UserCmd),
            ("LOGIN", MessageType::Login),
            ("AVC", MessageType::Avc),
            ("SELINUX_ERR", MessageType::SelinuxErr),
            ("DAEMON_ABORT", MessageType::DaemonAbort),
            ("DAEMON_CONFIG", MessageType::DaemonConfig),
            ("SERVICE_START", MessageType::ServiceStart),
            ("SERVICE_STOP", MessageType::ServiceStop),
        ];
        for (s, expected) in cases {
            assert_eq!(MessageType::from_str(s), expected, "failed for {s}");
        }
    }

    // -----------------------------------------------------------------------
    // RuleAction / RuleFilter tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_rule_action_from_str() {
        assert_eq!(RuleAction::from_str("always"), Some(RuleAction::Always));
        assert_eq!(RuleAction::from_str("never"), Some(RuleAction::Never));
        assert_eq!(RuleAction::from_str("invalid"), None);
    }

    #[test]
    fn test_rule_action_display() {
        assert_eq!(format!("{}", RuleAction::Always), "always");
        assert_eq!(format!("{}", RuleAction::Never), "never");
    }

    #[test]
    fn test_rule_filter_from_str() {
        assert_eq!(RuleFilter::from_str("task"), Some(RuleFilter::Task));
        assert_eq!(RuleFilter::from_str("exit"), Some(RuleFilter::Exit));
        assert_eq!(RuleFilter::from_str("user"), Some(RuleFilter::User));
        assert_eq!(RuleFilter::from_str("exclude"), Some(RuleFilter::Exclude));
        assert_eq!(RuleFilter::from_str("bad"), None);
    }

    #[test]
    fn test_rule_filter_display() {
        assert_eq!(format!("{}", RuleFilter::Exit), "exit");
        assert_eq!(format!("{}", RuleFilter::Task), "task");
    }

    // -----------------------------------------------------------------------
    // WatchPerms tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_watch_perms_from_str_all() {
        let p = WatchPerms::from_str("rwxa").unwrap();
        assert!(p.read && p.write && p.execute && p.attribute);
    }

    #[test]
    fn test_watch_perms_from_str_partial() {
        let p = WatchPerms::from_str("rw").unwrap();
        assert!(p.read && p.write && !p.execute && !p.attribute);
    }

    #[test]
    fn test_watch_perms_from_str_single() {
        let p = WatchPerms::from_str("x").unwrap();
        assert!(!p.read && !p.write && p.execute && !p.attribute);
    }

    #[test]
    fn test_watch_perms_from_str_invalid() {
        assert!(WatchPerms::from_str("rwz").is_none());
    }

    #[test]
    fn test_watch_perms_as_string() {
        let p = WatchPerms { read: true, write: false, execute: true, attribute: false };
        assert_eq!(p.as_string(), "rx");
    }

    #[test]
    fn test_watch_perms_display() {
        let p = WatchPerms { read: true, write: true, execute: false, attribute: true };
        assert_eq!(format!("{p}"), "rwa");
    }

    #[test]
    fn test_watch_perms_empty() {
        let p = WatchPerms::from_str("").unwrap();
        assert_eq!(p.as_string(), "");
    }

    #[test]
    fn test_watch_perms_roundtrip() {
        for combo in &["r", "w", "x", "a", "rw", "rx", "ra", "wx", "rwx", "rwxa"] {
            let p = WatchPerms::from_str(combo).unwrap();
            let s = p.as_string();
            let p2 = WatchPerms::from_str(&s).unwrap();
            assert_eq!(p, p2);
        }
    }

    // -----------------------------------------------------------------------
    // FieldFilter tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_field_filter_equals() {
        let f = parse_field_filter("uid=0").unwrap();
        assert_eq!(f.field, "uid");
        assert_eq!(f.op, "=");
        assert_eq!(f.value, "0");
    }

    #[test]
    fn test_parse_field_filter_not_equals() {
        let f = parse_field_filter("auid!=4294967295").unwrap();
        assert_eq!(f.field, "auid");
        assert_eq!(f.op, "!=");
        assert_eq!(f.value, "4294967295");
    }

    #[test]
    fn test_parse_field_filter_less_than() {
        let f = parse_field_filter("pid<1000").unwrap();
        assert_eq!(f.field, "pid");
        assert_eq!(f.op, "<");
        assert_eq!(f.value, "1000");
    }

    #[test]
    fn test_parse_field_filter_greater_equal() {
        let f = parse_field_filter("uid>=500").unwrap();
        assert_eq!(f.field, "uid");
        assert_eq!(f.op, ">=");
        assert_eq!(f.value, "500");
    }

    #[test]
    fn test_parse_field_filter_invalid() {
        assert!(parse_field_filter("noop").is_none());
        assert!(parse_field_filter("=value").is_none());
        assert!(parse_field_filter("field=").is_none());
    }

    #[test]
    fn test_field_filter_display() {
        let f = FieldFilter { field: "uid".into(), op: "=".into(), value: "0".into() };
        assert_eq!(format!("{f}"), "uid=0");
    }

    // -----------------------------------------------------------------------
    // AuditRule tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_watch_rule_display() {
        let r = AuditRule::Watch {
            path: "/etc/passwd".into(),
            perms: WatchPerms::from_str("rwa").unwrap(),
            key: Some("passwd_changes".into()),
        };
        assert_eq!(format!("{r}"), "-w /etc/passwd -p rwa -k passwd_changes");
    }

    #[test]
    fn test_watch_rule_display_no_key() {
        let r = AuditRule::Watch {
            path: "/tmp".into(),
            perms: WatchPerms::from_str("rwxa").unwrap(),
            key: None,
        };
        assert_eq!(format!("{r}"), "-w /tmp -p rwxa");
    }

    #[test]
    fn test_syscall_rule_display() {
        let r = AuditRule::Syscall {
            action: RuleAction::Always,
            filter: RuleFilter::Exit,
            syscall: Some("open".into()),
            fields: vec![
                FieldFilter { field: "uid".into(), op: "=".into(), value: "0".into() },
            ],
            key: Some("root_open".into()),
        };
        assert_eq!(format!("{r}"), "-a always,exit -S open -F uid=0 -k root_open");
    }

    #[test]
    fn test_syscall_rule_display_no_syscall() {
        let r = AuditRule::Syscall {
            action: RuleAction::Never,
            filter: RuleFilter::Exclude,
            syscall: None,
            fields: vec![],
            key: None,
        };
        assert_eq!(format!("{r}"), "-a never,exclude");
    }

    // -----------------------------------------------------------------------
    // RuleStore tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_rule_store_new_empty() {
        let store = RuleStore::new();
        assert!(store.rules.is_empty());
        assert_eq!(store.status.enabled, 1);
    }

    #[test]
    fn test_rule_store_add_watch() {
        let mut store = RuleStore::new();
        store.add_rule(AuditRule::Watch {
            path: "/etc".into(),
            perms: WatchPerms::from_str("rw").unwrap(),
            key: None,
        });
        assert_eq!(store.rules.len(), 1);
    }

    #[test]
    fn test_rule_store_add_syscall() {
        let mut store = RuleStore::new();
        store.add_rule(AuditRule::Syscall {
            action: RuleAction::Always,
            filter: RuleFilter::Exit,
            syscall: Some("write".into()),
            fields: vec![],
            key: None,
        });
        assert_eq!(store.rules.len(), 1);
    }

    #[test]
    fn test_rule_store_delete_all() {
        let mut store = RuleStore::new();
        store.add_rule(AuditRule::Watch {
            path: "/a".into(),
            perms: WatchPerms::default(),
            key: None,
        });
        store.add_rule(AuditRule::Watch {
            path: "/b".into(),
            perms: WatchPerms::default(),
            key: None,
        });
        let deleted = store.delete_all();
        assert_eq!(deleted, 2);
        assert!(store.rules.is_empty());
    }

    #[test]
    fn test_rule_store_delete_specific() {
        let mut store = RuleStore::new();
        store.add_rule(AuditRule::Syscall {
            action: RuleAction::Always,
            filter: RuleFilter::Exit,
            syscall: Some("open".into()),
            fields: vec![],
            key: None,
        });
        store.add_rule(AuditRule::Syscall {
            action: RuleAction::Always,
            filter: RuleFilter::Exit,
            syscall: Some("write".into()),
            fields: vec![],
            key: None,
        });
        assert!(store.delete_rule(RuleAction::Always, RuleFilter::Exit, Some("open")));
        assert_eq!(store.rules.len(), 1);
    }

    #[test]
    fn test_rule_store_delete_nonexistent() {
        let mut store = RuleStore::new();
        assert!(!store.delete_rule(RuleAction::Always, RuleFilter::Exit, Some("open")));
    }

    #[test]
    fn test_rule_store_list() {
        let mut store = RuleStore::new();
        store.add_rule(AuditRule::Watch {
            path: "/etc".into(),
            perms: WatchPerms::from_str("rw").unwrap(),
            key: Some("etc_watch".into()),
        });
        let rules = store.list_rules();
        assert_eq!(rules.len(), 1);
    }

    #[test]
    fn test_rule_store_delete_by_action_filter_only() {
        let mut store = RuleStore::new();
        store.add_rule(AuditRule::Syscall {
            action: RuleAction::Never,
            filter: RuleFilter::User,
            syscall: None,
            fields: vec![],
            key: None,
        });
        assert!(store.delete_rule(RuleAction::Never, RuleFilter::User, None));
        assert!(store.rules.is_empty());
    }

    // -----------------------------------------------------------------------
    // AuditStatus tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_audit_status_new() {
        let s = AuditStatus::new();
        assert_eq!(s.enabled, 1);
        assert_eq!(s.rate_limit, 0);
        assert_eq!(s.backlog_limit, 8192);
        assert_eq!(s.lost, 0);
        assert_eq!(s.backlog, 0);
    }

    #[test]
    fn test_audit_status_display() {
        let s = AuditStatus::new();
        let output = format!("{s}");
        assert!(output.contains("enabled 1"));
        assert!(output.contains("backlog_limit 8192"));
        assert!(output.contains("lost 0"));
    }

    // -----------------------------------------------------------------------
    // AuditRecord parsing tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_syscall_record() {
        let line = "type=SYSCALL msg=audit(1735689600.000:100): arch=c000003e syscall=open success=yes pid=1234 uid=0 auid=1000 exe=\"/usr/bin/cat\" key=\"file_access\"";
        let r = AuditRecord::parse(line).unwrap();
        assert_eq!(r.msg_type, MessageType::Syscall);
        assert_eq!(r.timestamp, 1_735_689_600.0);
        assert_eq!(r.serial, 100);
        assert_eq!(r.get_field("syscall"), Some("open"));
        assert_eq!(r.get_field("success"), Some("yes"));
        assert_eq!(r.get_field("pid"), Some("1234"));
        assert_eq!(r.get_field("uid"), Some("0"));
        assert_eq!(r.key(), Some("file_access"));
    }

    #[test]
    fn test_parse_user_auth_record() {
        let line = "type=USER_AUTH msg=audit(1735689700.000:101): pid=5678 uid=0 auid=1000 acct=\"testuser\" hostname=localhost terminal=ssh res=success";
        let r = AuditRecord::parse(line).unwrap();
        assert_eq!(r.msg_type, MessageType::UserAuth);
        assert_eq!(r.serial, 101);
        assert_eq!(r.acct(), Some("testuser"));
        assert_eq!(r.res(), Some("success"));
    }

    #[test]
    fn test_parse_path_record() {
        let line = "type=PATH msg=audit(1735689600.000:100): item=0 name=\"/etc/passwd\" nametype=NORMAL";
        let r = AuditRecord::parse(line).unwrap();
        assert_eq!(r.msg_type, MessageType::Path);
        assert_eq!(r.name_field(), Some("/etc/passwd"));
    }

    #[test]
    fn test_parse_cwd_record() {
        let line = "type=CWD msg=audit(1735689600.000:100): cwd=\"/home/user\"";
        let r = AuditRecord::parse(line).unwrap();
        assert_eq!(r.msg_type, MessageType::Cwd);
        assert_eq!(r.get_field("cwd"), Some("/home/user"));
    }

    #[test]
    fn test_parse_empty_line() {
        assert!(AuditRecord::parse("").is_none());
    }

    #[test]
    fn test_parse_invalid_line() {
        assert!(AuditRecord::parse("garbage data here").is_none());
    }

    #[test]
    fn test_parse_no_type_prefix() {
        assert!(AuditRecord::parse("msg=audit(123:1): foo=bar").is_none());
    }

    #[test]
    fn test_record_success_yes() {
        let line = "type=SYSCALL msg=audit(1735689600.000:1): success=yes syscall=read";
        let r = AuditRecord::parse(line).unwrap();
        assert_eq!(r.success(), Some(true));
    }

    #[test]
    fn test_record_success_no() {
        let line = "type=SYSCALL msg=audit(1735689600.000:1): success=no syscall=read";
        let r = AuditRecord::parse(line).unwrap();
        assert_eq!(r.success(), Some(false));
    }

    #[test]
    fn test_record_success_absent() {
        let line = "type=PATH msg=audit(1735689600.000:1): name=\"/etc/shadow\"";
        let r = AuditRecord::parse(line).unwrap();
        assert_eq!(r.success(), None);
    }

    #[test]
    fn test_record_syscall_name() {
        let line = "type=SYSCALL msg=audit(1735689600.000:1): syscall=openat success=yes";
        let r = AuditRecord::parse(line).unwrap();
        assert_eq!(r.syscall_name(), Some("openat"));
    }

    #[test]
    fn test_record_exe() {
        let line = "type=SYSCALL msg=audit(1735689600.000:1): exe=\"/bin/ls\" syscall=stat";
        let r = AuditRecord::parse(line).unwrap();
        assert_eq!(r.exe(), Some("/bin/ls"));
    }

    #[test]
    fn test_record_auid() {
        let line = "type=SYSCALL msg=audit(1735689600.000:1): auid=1000 uid=0";
        let r = AuditRecord::parse(line).unwrap();
        assert_eq!(r.auid(), Some("1000"));
        assert_eq!(r.uid(), Some("0"));
    }

    #[test]
    fn test_record_terminal() {
        let line = "type=USER_AUTH msg=audit(1735689600.000:1): terminal=pts/0 res=success";
        let r = AuditRecord::parse(line).unwrap();
        assert_eq!(r.terminal(), Some("pts/0"));
    }

    #[test]
    fn test_record_pid() {
        let line = "type=SYSCALL msg=audit(1735689600.000:1): pid=42 syscall=fork";
        let r = AuditRecord::parse(line).unwrap();
        assert_eq!(r.pid(), Some("42"));
    }

    #[test]
    fn test_record_format_timestamp() {
        let line = "type=SYSCALL msg=audit(1735689600.000:1): syscall=read";
        let r = AuditRecord::parse(line).unwrap();
        let ts = r.format_timestamp();
        assert!(ts.contains("2025/01/01"));
    }

    // -----------------------------------------------------------------------
    // parse_field_pairs tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_field_pairs_simple() {
        let mut fields = HashMap::new();
        parse_field_pairs("a=1 b=2 c=3", &mut fields);
        assert_eq!(fields.get("a"), Some(&"1".to_string()));
        assert_eq!(fields.get("b"), Some(&"2".to_string()));
        assert_eq!(fields.get("c"), Some(&"3".to_string()));
    }

    #[test]
    fn test_parse_field_pairs_quoted() {
        let mut fields = HashMap::new();
        parse_field_pairs("name=\"hello world\" val=42", &mut fields);
        assert_eq!(fields.get("name"), Some(&"hello world".to_string()));
        assert_eq!(fields.get("val"), Some(&"42".to_string()));
    }

    #[test]
    fn test_parse_field_pairs_empty() {
        let mut fields = HashMap::new();
        parse_field_pairs("", &mut fields);
        assert!(fields.is_empty());
    }

    #[test]
    fn test_parse_field_pairs_single() {
        let mut fields = HashMap::new();
        parse_field_pairs("key=value", &mut fields);
        assert_eq!(fields.get("key"), Some(&"value".to_string()));
    }

    #[test]
    fn test_parse_field_pairs_unterminated_quote() {
        let mut fields = HashMap::new();
        parse_field_pairs("key=\"unterminated", &mut fields);
        assert_eq!(fields.get("key"), Some(&"unterminated".to_string()));
    }

    // -----------------------------------------------------------------------
    // Date/time conversion tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_days_to_ymd_epoch() {
        let (y, m, d) = days_to_ymd(0);
        assert_eq!((y, m, d), (1970, 1, 1));
    }

    #[test]
    fn test_days_to_ymd_2025() {
        // 2025-01-01 = day 20089
        let (y, m, d) = days_to_ymd(20089);
        assert_eq!((y, m, d), (2025, 1, 1));
    }

    #[test]
    fn test_days_to_ymd_leap_year() {
        // 2000-02-29 = day 11016
        let (y, m, d) = days_to_ymd(11016);
        assert_eq!((y, m, d), (2000, 2, 29));
    }

    #[test]
    fn test_ymd_to_days_epoch() {
        assert_eq!(ymd_to_days(1970, 1, 1), Some(0));
    }

    #[test]
    fn test_ymd_to_days_roundtrip() {
        for days in [0u64, 100, 365, 1000, 10000, 20000, 20089] {
            let (y, m, d) = days_to_ymd(days);
            assert_eq!(ymd_to_days(y, m, d), Some(days));
        }
    }

    #[test]
    fn test_ymd_to_days_invalid_month() {
        assert_eq!(ymd_to_days(2025, 0, 1), None);
        assert_eq!(ymd_to_days(2025, 13, 1), None);
    }

    #[test]
    fn test_ymd_to_days_invalid_day() {
        assert_eq!(ymd_to_days(2025, 1, 0), None);
        assert_eq!(ymd_to_days(2025, 1, 32), None);
    }

    #[test]
    fn test_format_epoch_secs() {
        let s = format_epoch_secs(0);
        assert_eq!(s, "1970/01/01 00:00:00");
    }

    #[test]
    fn test_format_epoch_secs_2025() {
        let s = format_epoch_secs(1_735_689_600);
        assert!(s.starts_with("2025/01/01"));
    }

    // -----------------------------------------------------------------------
    // parse_time_spec tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_time_spec_epoch() {
        assert_eq!(parse_time_spec("1735689600"), Some(1_735_689_600.0));
    }

    #[test]
    fn test_parse_time_spec_epoch_float() {
        assert_eq!(parse_time_spec("1735689600.5"), Some(1_735_689_600.5));
    }

    #[test]
    fn test_parse_time_spec_now() {
        // Uses simulated_now(), default 1735689600.0
        let t = parse_time_spec("now").unwrap();
        assert!(t > 0.0);
    }

    #[test]
    fn test_parse_time_spec_today() {
        let t = parse_time_spec("today").unwrap();
        assert!(t > 0.0);
    }

    #[test]
    fn test_parse_time_spec_yesterday() {
        let t = parse_time_spec("yesterday").unwrap();
        let today = parse_time_spec("today").unwrap();
        assert_eq!(today - t, 86400.0);
    }

    #[test]
    fn test_parse_time_spec_this_week() {
        let t = parse_time_spec("this-week").unwrap();
        let today = parse_time_spec("today").unwrap();
        assert_eq!(today - t, 7.0 * 86400.0);
    }

    #[test]
    fn test_parse_time_spec_this_month() {
        let t = parse_time_spec("this-month").unwrap();
        let today = parse_time_spec("today").unwrap();
        assert_eq!(today - t, 30.0 * 86400.0);
    }

    #[test]
    fn test_parse_time_spec_date_format() {
        let t = parse_time_spec("1970/01/01 00:00:00").unwrap();
        assert_eq!(t, 0.0);
    }

    #[test]
    fn test_parse_time_spec_date_dash_format() {
        let t = parse_time_spec("1970-01-01 00:00:00").unwrap();
        assert_eq!(t, 0.0);
    }

    #[test]
    fn test_parse_time_spec_date_only() {
        let t = parse_time_spec("2025/01/01").unwrap();
        assert_eq!(t, 1_735_689_600.0);
    }

    #[test]
    fn test_parse_time_spec_invalid() {
        assert!(parse_time_spec("not-a-date").is_none());
    }

    // -----------------------------------------------------------------------
    // load_audit_log_from_str tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_load_audit_log_from_str_empty() {
        let records = load_audit_log_from_str("");
        assert!(records.is_empty());
    }

    #[test]
    fn test_load_audit_log_from_str_single() {
        let log = "type=SYSCALL msg=audit(1735689600.000:1): syscall=open success=yes pid=1";
        let records = load_audit_log_from_str(log);
        assert_eq!(records.len(), 1);
    }

    #[test]
    fn test_load_audit_log_from_str_multiple() {
        let log = "\
type=SYSCALL msg=audit(1735689600.000:1): syscall=open success=yes
type=PATH msg=audit(1735689600.000:1): name=\"/etc/passwd\"
type=CWD msg=audit(1735689600.000:1): cwd=\"/home\"";
        let records = load_audit_log_from_str(log);
        assert_eq!(records.len(), 3);
    }

    #[test]
    fn test_load_audit_log_from_str_skips_invalid() {
        let log = "\
type=SYSCALL msg=audit(1735689600.000:1): syscall=open
garbage line here
type=PATH msg=audit(1735689600.000:2): name=\"/tmp\"";
        let records = load_audit_log_from_str(log);
        assert_eq!(records.len(), 2);
    }

    // -----------------------------------------------------------------------
    // Search criteria / record matching tests
    // -----------------------------------------------------------------------

    fn make_syscall_record(syscall: &str, success: &str, key: &str, uid: &str, ts: f64) -> AuditRecord {
        let line = format!(
            "type=SYSCALL msg=audit({:.3}:{}): syscall={} success={} uid={} auid={} key=\"{}\" pid=100 exe=\"/bin/test\"",
            ts,
            (ts * 1000.0) as u64 % 1_000_000,
            syscall,
            success,
            uid,
            uid,
            key,
        );
        AuditRecord::parse(&line).unwrap()
    }

    fn make_auth_record(acct: &str, res: &str, ts: f64) -> AuditRecord {
        let line = format!(
            "type=USER_AUTH msg=audit({:.3}:{}): pid=1 uid=0 auid=1000 acct=\"{}\" hostname=localhost terminal=pts/0 res={}",
            ts,
            (ts * 1000.0) as u64 % 1_000_000,
            acct,
            res,
        );
        AuditRecord::parse(&line).unwrap()
    }

    #[test]
    fn test_record_matches_no_criteria() {
        let r = make_syscall_record("open", "yes", "test", "1000", 1_735_689_600.0);
        let c = SearchCriteria::default();
        assert!(record_matches(&r, &c));
    }

    #[test]
    fn test_record_matches_by_key() {
        let r = make_syscall_record("open", "yes", "file_access", "1000", 1_735_689_600.0);
        let c = SearchCriteria { key: Some("file_access".into()), ..Default::default() };
        assert!(record_matches(&r, &c));
    }

    #[test]
    fn test_record_no_match_wrong_key() {
        let r = make_syscall_record("open", "yes", "file_access", "1000", 1_735_689_600.0);
        let c = SearchCriteria { key: Some("wrong_key".into()), ..Default::default() };
        assert!(!record_matches(&r, &c));
    }

    #[test]
    fn test_record_matches_by_syscall() {
        let r = make_syscall_record("open", "yes", "test", "1000", 1_735_689_600.0);
        let c = SearchCriteria { syscall: Some("open".into()), ..Default::default() };
        assert!(record_matches(&r, &c));
    }

    #[test]
    fn test_record_no_match_wrong_syscall() {
        let r = make_syscall_record("open", "yes", "test", "1000", 1_735_689_600.0);
        let c = SearchCriteria { syscall: Some("write".into()), ..Default::default() };
        assert!(!record_matches(&r, &c));
    }

    #[test]
    fn test_record_matches_by_success() {
        let r = make_syscall_record("open", "yes", "test", "1000", 1_735_689_600.0);
        let c = SearchCriteria { success: Some(true), ..Default::default() };
        assert!(record_matches(&r, &c));
    }

    #[test]
    fn test_record_no_match_wrong_success() {
        let r = make_syscall_record("open", "yes", "test", "1000", 1_735_689_600.0);
        let c = SearchCriteria { success: Some(false), ..Default::default() };
        assert!(!record_matches(&r, &c));
    }

    #[test]
    fn test_record_matches_by_user() {
        let r = make_syscall_record("open", "yes", "test", "1000", 1_735_689_600.0);
        let c = SearchCriteria { user: Some("1000".into()), ..Default::default() };
        assert!(record_matches(&r, &c));
    }

    #[test]
    fn test_record_no_match_wrong_user() {
        let r = make_syscall_record("open", "yes", "test", "1000", 1_735_689_600.0);
        let c = SearchCriteria { user: Some("9999".into()), ..Default::default() };
        assert!(!record_matches(&r, &c));
    }

    #[test]
    fn test_record_matches_by_time_range() {
        let r = make_syscall_record("open", "yes", "test", "1000", 1_735_689_700.0);
        let c = SearchCriteria {
            start_time: Some(1_735_689_600.0),
            end_time: Some(1_735_689_800.0),
            ..Default::default()
        };
        assert!(record_matches(&r, &c));
    }

    #[test]
    fn test_record_no_match_before_start() {
        let r = make_syscall_record("open", "yes", "test", "1000", 1_735_689_500.0);
        let c = SearchCriteria {
            start_time: Some(1_735_689_600.0),
            ..Default::default()
        };
        assert!(!record_matches(&r, &c));
    }

    #[test]
    fn test_record_no_match_after_end() {
        let r = make_syscall_record("open", "yes", "test", "1000", 1_735_689_900.0);
        let c = SearchCriteria {
            end_time: Some(1_735_689_800.0),
            ..Default::default()
        };
        assert!(!record_matches(&r, &c));
    }

    #[test]
    fn test_record_matches_by_message_type() {
        let r = make_syscall_record("open", "yes", "test", "1000", 1_735_689_600.0);
        let c = SearchCriteria {
            message_type: Some(MessageType::Syscall),
            ..Default::default()
        };
        assert!(record_matches(&r, &c));
    }

    #[test]
    fn test_record_no_match_wrong_message_type() {
        let r = make_syscall_record("open", "yes", "test", "1000", 1_735_689_600.0);
        let c = SearchCriteria {
            message_type: Some(MessageType::UserAuth),
            ..Default::default()
        };
        assert!(!record_matches(&r, &c));
    }

    #[test]
    fn test_record_matches_user_by_acct() {
        let r = make_auth_record("alice", "success", 1_735_689_600.0);
        let c = SearchCriteria { user: Some("alice".into()), ..Default::default() };
        assert!(record_matches(&r, &c));
    }

    #[test]
    fn test_record_matches_combined_criteria() {
        let r = make_syscall_record("open", "yes", "file_access", "1000", 1_735_689_700.0);
        let c = SearchCriteria {
            key: Some("file_access".into()),
            syscall: Some("open".into()),
            success: Some(true),
            user: Some("1000".into()),
            start_time: Some(1_735_689_600.0),
            end_time: Some(1_735_689_800.0),
            message_type: Some(MessageType::Syscall),
            ..Default::default()
        };
        assert!(record_matches(&r, &c));
    }

    // -----------------------------------------------------------------------
    // format_record tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_format_record_raw() {
        let line = "type=SYSCALL msg=audit(1735689600.000:1): syscall=open success=yes";
        let r = AuditRecord::parse(line).unwrap();
        let out = format_record(&r, &OutputFormat::Raw, false);
        assert_eq!(out, line);
    }

    #[test]
    fn test_format_record_text() {
        let line = "type=SYSCALL msg=audit(1735689600.000:1): syscall=open";
        let r = AuditRecord::parse(line).unwrap();
        let out = format_record(&r, &OutputFormat::Text, false);
        assert!(out.contains("serial=1"));
        assert!(out.contains("type=SYSCALL"));
    }

    #[test]
    fn test_format_record_csv() {
        let line = "type=SYSCALL msg=audit(1735689600.000:1): syscall=open pid=42 success=yes";
        let r = AuditRecord::parse(line).unwrap();
        let out = format_record(&r, &OutputFormat::Csv, false);
        assert!(out.contains("SYSCALL"));
        assert!(out.contains(",1,"));
    }

    // -----------------------------------------------------------------------
    // OutputFormat tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_output_format_from_str() {
        assert_eq!(OutputFormat::from_str("raw"), Some(OutputFormat::Raw));
        assert_eq!(OutputFormat::from_str("text"), Some(OutputFormat::Text));
        assert_eq!(OutputFormat::from_str("csv"), Some(OutputFormat::Csv));
        assert_eq!(OutputFormat::from_str("xml"), None);
    }

    // -----------------------------------------------------------------------
    // AuditdConfig tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_auditd_config_defaults() {
        let c = AuditdConfig::new();
        assert_eq!(c.log_file, DEFAULT_LOG_FILE);
        assert_eq!(c.max_log_file, 8);
        assert_eq!(c.num_logs, 5);
        assert_eq!(c.max_log_file_action, "ROTATE");
        assert!(c.write_logs);
    }

    #[test]
    fn test_auditd_config_apply_setting() {
        let mut c = AuditdConfig::new();
        c.apply_setting("log_file", "/tmp/audit.log");
        assert_eq!(c.log_file, "/tmp/audit.log");
    }

    #[test]
    fn test_auditd_config_apply_numeric() {
        let mut c = AuditdConfig::new();
        c.apply_setting("max_log_file", "16");
        assert_eq!(c.max_log_file, 16);
    }

    #[test]
    fn test_auditd_config_apply_num_logs() {
        let mut c = AuditdConfig::new();
        c.apply_setting("num_logs", "10");
        assert_eq!(c.num_logs, 10);
    }

    #[test]
    fn test_auditd_config_apply_write_logs() {
        let mut c = AuditdConfig::new();
        c.apply_setting("write_logs", "no");
        assert!(!c.write_logs);
        c.apply_setting("write_logs", "yes");
        assert!(c.write_logs);
    }

    #[test]
    fn test_auditd_config_apply_all_settings() {
        let mut c = AuditdConfig::new();
        c.apply_setting("log_format", "RAW");
        c.apply_setting("space_left", "100");
        c.apply_setting("space_left_action", "EMAIL");
        c.apply_setting("admin_space_left", "25");
        c.apply_setting("admin_space_left_action", "HALT");
        c.apply_setting("disk_full_action", "HALT");
        c.apply_setting("disk_error_action", "HALT");
        c.apply_setting("flush", "SYNC");
        c.apply_setting("freq", "100");
        c.apply_setting("priority_boost", "6");
        c.apply_setting("disp_qos", "lossless");
        c.apply_setting("dispatcher", "/sbin/mydispatcher");
        c.apply_setting("name_format", "HOSTNAME");
        c.apply_setting("name", "myhost");
        c.apply_setting("tcp_listen_port", "60");
        c.apply_setting("tcp_max_per_addr", "5");

        assert_eq!(c.log_format, "RAW");
        assert_eq!(c.space_left, 100);
        assert_eq!(c.space_left_action, "EMAIL");
        assert_eq!(c.admin_space_left, 25);
        assert_eq!(c.admin_space_left_action, "HALT");
        assert_eq!(c.disk_full_action, "HALT");
        assert_eq!(c.disk_error_action, "HALT");
        assert_eq!(c.flush, "SYNC");
        assert_eq!(c.freq, 100);
        assert_eq!(c.priority_boost, 6);
        assert_eq!(c.disp_qos, "lossless");
        assert_eq!(c.dispatcher, "/sbin/mydispatcher");
        assert_eq!(c.name_format, "HOSTNAME");
        assert_eq!(c.name, "myhost");
        assert_eq!(c.tcp_listen_port, 60);
        assert_eq!(c.tcp_max_per_addr, 5);
    }

    #[test]
    fn test_auditd_config_apply_invalid_numeric() {
        let mut c = AuditdConfig::new();
        c.apply_setting("max_log_file", "not_a_number");
        assert_eq!(c.max_log_file, 8); // unchanged
    }

    #[test]
    fn test_auditd_config_apply_unknown_key() {
        let mut c = AuditdConfig::new();
        let old_log_file = c.log_file.clone();
        c.apply_setting("unknown_key", "value");
        assert_eq!(c.log_file, old_log_file); // nothing changed
    }

    #[test]
    fn test_auditd_config_display() {
        let c = AuditdConfig::new();
        let output = format!("{c}");
        assert!(output.contains("log_file = /var/log/audit/audit.log"));
        assert!(output.contains("max_log_file = 8"));
        assert!(output.contains("num_logs = 5"));
        assert!(output.contains("write_logs = yes"));
    }

    #[test]
    fn test_auditd_config_parse_nonexistent() {
        let c = AuditdConfig::parse_config_file("/nonexistent/path/config");
        assert_eq!(c.log_file, DEFAULT_LOG_FILE); // defaults
    }

    // -----------------------------------------------------------------------
    // auditctl argument processing tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_auditctl_add_watch_rule() {
        let mut store = RuleStore::new();
        let args: Vec<String> = vec!["-w", "/etc/passwd", "-p", "rwa", "-k", "passwd"]
            .into_iter().map(|s| s.to_string()).collect();
        let rc = process_auditctl_args(&args, &mut store);
        assert_eq!(rc, 0);
        assert_eq!(store.rules.len(), 1);
        if let AuditRule::Watch { ref path, ref perms, ref key } = store.rules[0] {
            assert_eq!(path, "/etc/passwd");
            assert!(perms.read && perms.write && !perms.execute && perms.attribute);
            assert_eq!(key.as_deref(), Some("passwd"));
        } else {
            panic!("Expected Watch rule");
        }
    }

    #[test]
    fn test_auditctl_add_syscall_rule() {
        let mut store = RuleStore::new();
        let args: Vec<String> = vec!["-a", "always,exit", "-S", "open", "-F", "uid=0", "-k", "root_open"]
            .into_iter().map(|s| s.to_string()).collect();
        let rc = process_auditctl_args(&args, &mut store);
        assert_eq!(rc, 0);
        assert_eq!(store.rules.len(), 1);
    }

    #[test]
    fn test_auditctl_delete_rule() {
        let mut store = RuleStore::new();
        store.add_rule(AuditRule::Syscall {
            action: RuleAction::Always,
            filter: RuleFilter::Exit,
            syscall: Some("open".into()),
            fields: vec![],
            key: None,
        });
        let args: Vec<String> = vec!["-d", "always,exit", "-S", "open"]
            .into_iter().map(|s| s.to_string()).collect();
        let rc = process_auditctl_args(&args, &mut store);
        assert_eq!(rc, 0);
        assert!(store.rules.is_empty());
    }

    #[test]
    fn test_auditctl_delete_all() {
        let mut store = RuleStore::new();
        store.add_rule(AuditRule::Watch {
            path: "/a".into(),
            perms: WatchPerms::default(),
            key: None,
        });
        store.add_rule(AuditRule::Watch {
            path: "/b".into(),
            perms: WatchPerms::default(),
            key: None,
        });
        let args: Vec<String> = vec!["-D"].into_iter().map(|s| s.to_string()).collect();
        let rc = process_auditctl_args(&args, &mut store);
        assert_eq!(rc, 0);
        assert!(store.rules.is_empty());
    }

    #[test]
    fn test_auditctl_enable() {
        let mut store = RuleStore::new();
        let args: Vec<String> = vec!["-e", "0"].into_iter().map(|s| s.to_string()).collect();
        let rc = process_auditctl_args(&args, &mut store);
        assert_eq!(rc, 0);
        assert_eq!(store.status.enabled, 0);
    }

    #[test]
    fn test_auditctl_enable_lock() {
        let mut store = RuleStore::new();
        let args: Vec<String> = vec!["-e", "2"].into_iter().map(|s| s.to_string()).collect();
        let rc = process_auditctl_args(&args, &mut store);
        assert_eq!(rc, 0);
        assert_eq!(store.status.enabled, 2);
    }

    #[test]
    fn test_auditctl_backlog_limit() {
        let mut store = RuleStore::new();
        let args: Vec<String> = vec!["-b", "16384"].into_iter().map(|s| s.to_string()).collect();
        let rc = process_auditctl_args(&args, &mut store);
        assert_eq!(rc, 0);
        assert_eq!(store.status.backlog_limit, 16384);
    }

    #[test]
    fn test_auditctl_rate_limit() {
        let mut store = RuleStore::new();
        let args: Vec<String> = vec!["-r", "100"].into_iter().map(|s| s.to_string()).collect();
        let rc = process_auditctl_args(&args, &mut store);
        assert_eq!(rc, 0);
        assert_eq!(store.status.rate_limit, 100);
    }

    #[test]
    fn test_auditctl_invalid_action() {
        let mut store = RuleStore::new();
        let args: Vec<String> = vec!["-a", "invalid,exit"]
            .into_iter().map(|s| s.to_string()).collect();
        let rc = process_auditctl_args(&args, &mut store);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_auditctl_invalid_filter() {
        let mut store = RuleStore::new();
        let args: Vec<String> = vec!["-a", "always,invalid"]
            .into_iter().map(|s| s.to_string()).collect();
        let rc = process_auditctl_args(&args, &mut store);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_auditctl_invalid_perms() {
        let mut store = RuleStore::new();
        let args: Vec<String> = vec!["-w", "/tmp", "-p", "rwz"]
            .into_iter().map(|s| s.to_string()).collect();
        let rc = process_auditctl_args(&args, &mut store);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_auditctl_invalid_enable_value() {
        let mut store = RuleStore::new();
        let args: Vec<String> = vec!["-e", "5"].into_iter().map(|s| s.to_string()).collect();
        let rc = process_auditctl_args(&args, &mut store);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_auditctl_invalid_backlog() {
        let mut store = RuleStore::new();
        let args: Vec<String> = vec!["-b", "abc"].into_iter().map(|s| s.to_string()).collect();
        let rc = process_auditctl_args(&args, &mut store);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_auditctl_invalid_rate() {
        let mut store = RuleStore::new();
        let args: Vec<String> = vec!["-r", "xyz"].into_iter().map(|s| s.to_string()).collect();
        let rc = process_auditctl_args(&args, &mut store);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_auditctl_unknown_option() {
        let mut store = RuleStore::new();
        let args: Vec<String> = vec!["--nonexistent"].into_iter().map(|s| s.to_string()).collect();
        let rc = process_auditctl_args(&args, &mut store);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_auditctl_missing_w_arg() {
        let mut store = RuleStore::new();
        let args: Vec<String> = vec!["-w"].into_iter().map(|s| s.to_string()).collect();
        let rc = process_auditctl_args(&args, &mut store);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_auditctl_missing_a_arg() {
        let mut store = RuleStore::new();
        let args: Vec<String> = vec!["-a"].into_iter().map(|s| s.to_string()).collect();
        let rc = process_auditctl_args(&args, &mut store);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_auditctl_missing_d_arg() {
        let mut store = RuleStore::new();
        let args: Vec<String> = vec!["-d"].into_iter().map(|s| s.to_string()).collect();
        let rc = process_auditctl_args(&args, &mut store);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_auditctl_missing_e_arg() {
        let mut store = RuleStore::new();
        let args: Vec<String> = vec!["-e"].into_iter().map(|s| s.to_string()).collect();
        let rc = process_auditctl_args(&args, &mut store);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_auditctl_missing_b_arg() {
        let mut store = RuleStore::new();
        let args: Vec<String> = vec!["-b"].into_iter().map(|s| s.to_string()).collect();
        let rc = process_auditctl_args(&args, &mut store);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_auditctl_missing_r_arg() {
        let mut store = RuleStore::new();
        let args: Vec<String> = vec!["-r"].into_iter().map(|s| s.to_string()).collect();
        let rc = process_auditctl_args(&args, &mut store);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_auditctl_no_comma_in_action_filter() {
        let mut store = RuleStore::new();
        let args: Vec<String> = vec!["-a", "alwaysexit"]
            .into_iter().map(|s| s.to_string()).collect();
        let rc = process_auditctl_args(&args, &mut store);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_auditctl_help_returns_zero() {
        let mut store = RuleStore::new();
        let args: Vec<String> = vec!["-h"].into_iter().map(|s| s.to_string()).collect();
        let rc = process_auditctl_args(&args, &mut store);
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_auditctl_version_returns_zero() {
        let mut store = RuleStore::new();
        let args: Vec<String> = vec!["-v"].into_iter().map(|s| s.to_string()).collect();
        let rc = process_auditctl_args(&args, &mut store);
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_auditctl_empty_args() {
        let mut store = RuleStore::new();
        let args: Vec<String> = vec![];
        let rc = process_auditctl_args(&args, &mut store);
        assert_eq!(rc, 0); // shows help
    }

    #[test]
    fn test_auditctl_multiple_rules() {
        let mut store = RuleStore::new();
        let args1: Vec<String> = vec!["-w", "/etc/shadow", "-p", "rw", "-k", "shadow"]
            .into_iter().map(|s| s.to_string()).collect();
        process_auditctl_args(&args1, &mut store);

        let args2: Vec<String> = vec!["-a", "always,exit", "-S", "mount", "-k", "mounts"]
            .into_iter().map(|s| s.to_string()).collect();
        process_auditctl_args(&args2, &mut store);

        assert_eq!(store.rules.len(), 2);
    }

    #[test]
    fn test_auditctl_syscall_with_multiple_fields() {
        let mut store = RuleStore::new();
        let args: Vec<String> = vec![
            "-a", "always,exit", "-S", "open",
            "-F", "uid=0", "-F", "auid!=4294967295",
            "-k", "root_open",
        ].into_iter().map(|s| s.to_string()).collect();
        let rc = process_auditctl_args(&args, &mut store);
        assert_eq!(rc, 0);
        if let AuditRule::Syscall { ref fields, .. } = store.rules[0] {
            assert_eq!(fields.len(), 2);
        } else {
            panic!("Expected Syscall rule");
        }
    }

    #[test]
    fn test_auditctl_delete_nonexistent_returns_error() {
        let mut store = RuleStore::new();
        let args: Vec<String> = vec!["-d", "always,exit", "-S", "nonexistent"]
            .into_iter().map(|s| s.to_string()).collect();
        let rc = process_auditctl_args(&args, &mut store);
        assert_eq!(rc, 1);
    }

    // -----------------------------------------------------------------------
    // filter_by_time tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_filter_by_time_no_bounds() {
        let records = vec![
            make_syscall_record("open", "yes", "k", "0", 100.0),
            make_syscall_record("read", "yes", "k", "0", 200.0),
        ];
        let filtered = filter_by_time(&records, None, None);
        assert_eq!(filtered.len(), 2);
    }

    #[test]
    fn test_filter_by_time_start_only() {
        let records = vec![
            make_syscall_record("open", "yes", "k", "0", 100.0),
            make_syscall_record("read", "yes", "k", "0", 200.0),
        ];
        let filtered = filter_by_time(&records, Some(150.0), None);
        assert_eq!(filtered.len(), 1);
    }

    #[test]
    fn test_filter_by_time_end_only() {
        let records = vec![
            make_syscall_record("open", "yes", "k", "0", 100.0),
            make_syscall_record("read", "yes", "k", "0", 200.0),
        ];
        let filtered = filter_by_time(&records, None, Some(150.0));
        assert_eq!(filtered.len(), 1);
    }

    #[test]
    fn test_filter_by_time_both_bounds() {
        let records = vec![
            make_syscall_record("a", "yes", "k", "0", 100.0),
            make_syscall_record("b", "yes", "k", "0", 200.0),
            make_syscall_record("c", "yes", "k", "0", 300.0),
        ];
        let filtered = filter_by_time(&records, Some(150.0), Some(250.0));
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].timestamp, 200.0);
    }

    // -----------------------------------------------------------------------
    // Personality detection tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_personality_auditctl() {
        let name = extract_personality("/usr/bin/auditctl");
        assert_eq!(name, "auditctl");
    }

    #[test]
    fn test_personality_auditd() {
        let name = extract_personality("/usr/sbin/auditd");
        assert_eq!(name, "auditd");
    }

    #[test]
    fn test_personality_ausearch() {
        let name = extract_personality("/usr/bin/ausearch");
        assert_eq!(name, "ausearch");
    }

    #[test]
    fn test_personality_aureport() {
        let name = extract_personality("aureport");
        assert_eq!(name, "aureport");
    }

    #[test]
    fn test_personality_autrace() {
        let name = extract_personality("C:\\tools\\autrace.exe");
        assert_eq!(name, "autrace");
    }

    #[test]
    fn test_personality_windows_path() {
        let name = extract_personality("C:\\Program Files\\audit\\auditd.exe");
        assert_eq!(name, "auditd");
    }

    #[test]
    fn test_personality_unknown_defaults() {
        let name = extract_personality("/usr/bin/unknown");
        assert_eq!(name, "unknown");
    }

    /// Helper to test personality extraction using the same logic as main().
    fn extract_personality(s: &str) -> String {
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' {
                last_sep = i + 1;
            }
        }
        let base = &s[last_sep..];
        let base = base.strip_suffix(".exe").unwrap_or(base);
        base.to_string()
    }

    // -----------------------------------------------------------------------
    // ausearch argument edge case tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_ausearch_help() {
        let rc = run_ausearch(&["-h".to_string()]);
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_ausearch_version() {
        let rc = run_ausearch(&["-v".to_string()]);
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_ausearch_unknown_option() {
        let rc = run_ausearch(&["--bogus".to_string()]);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_ausearch_missing_key_arg() {
        let rc = run_ausearch(&["-k".to_string()]);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_ausearch_missing_sc_arg() {
        let rc = run_ausearch(&["-sc".to_string()]);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_ausearch_missing_sv_arg() {
        let rc = run_ausearch(&["-sv".to_string()]);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_ausearch_invalid_sv_arg() {
        let rc = run_ausearch(&["-sv".to_string(), "maybe".to_string()]);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_ausearch_missing_ua_arg() {
        let rc = run_ausearch(&["-ua".to_string()]);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_ausearch_missing_ts_arg() {
        let rc = run_ausearch(&["-ts".to_string()]);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_ausearch_missing_te_arg() {
        let rc = run_ausearch(&["-te".to_string()]);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_ausearch_missing_m_arg() {
        let rc = run_ausearch(&["-m".to_string()]);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_ausearch_missing_format_arg() {
        let rc = run_ausearch(&["--format".to_string()]);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_ausearch_invalid_format() {
        let rc = run_ausearch(&["--format".to_string(), "xml".to_string()]);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_ausearch_missing_input_arg() {
        let rc = run_ausearch(&["-if".to_string()]);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_ausearch_invalid_ts() {
        let rc = run_ausearch(&["-ts".to_string(), "not-a-date".to_string()]);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_ausearch_sv_success_alias() {
        // Just verifying parse doesn't crash — the actual log file won't exist.
        let rc = run_ausearch(&["-sv".to_string(), "success".to_string(), "-if".to_string(), "/nonexistent".to_string()]);
        // Returns 1 because no matches (no file), but no crash
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_ausearch_sv_failed_alias() {
        let rc = run_ausearch(&["-sv".to_string(), "failed".to_string(), "-if".to_string(), "/nonexistent".to_string()]);
        assert_eq!(rc, 1);
    }

    // -----------------------------------------------------------------------
    // aureport argument edge case tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_aureport_help() {
        let rc = run_aureport(&["-h".to_string()]);
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_aureport_version() {
        let rc = run_aureport(&["-v".to_string()]);
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_aureport_unknown_option() {
        let rc = run_aureport(&["--bogus".to_string()]);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_aureport_missing_ts_arg() {
        let rc = run_aureport(&["-ts".to_string()]);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_aureport_missing_te_arg() {
        let rc = run_aureport(&["-te".to_string()]);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_aureport_missing_input_arg() {
        let rc = run_aureport(&["-if".to_string()]);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_aureport_invalid_ts() {
        let rc = run_aureport(&["-ts".to_string(), "not-a-date".to_string()]);
        assert_eq!(rc, 1);
    }

    // -----------------------------------------------------------------------
    // autrace tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_autrace_help() {
        let rc = run_autrace(&["-h".to_string()]);
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_autrace_version() {
        let rc = run_autrace(&["-v".to_string()]);
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_autrace_no_args() {
        let rc = run_autrace(&[]);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_autrace_delete_rules() {
        let rc = run_autrace(&["-r".to_string()]);
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_autrace_program() {
        let rc = run_autrace(&["/bin/ls".to_string()]);
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_autrace_program_with_args() {
        let rc = run_autrace(&["/bin/ls".to_string(), "-la".to_string()]);
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_autrace_detach() {
        let rc = run_autrace(&["-d".to_string(), "/bin/ls".to_string()]);
        assert_eq!(rc, 0);
    }

    // -----------------------------------------------------------------------
    // auditd tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_auditd_help() {
        let rc = run_auditd(&["-h".to_string()]);
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_auditd_version() {
        let rc = run_auditd(&["-v".to_string()]);
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_auditd_unknown_option() {
        let rc = run_auditd(&["--bogus".to_string()]);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_auditd_missing_config_arg() {
        let rc = run_auditd(&["-c".to_string()]);
        assert_eq!(rc, 1);
    }

    // -----------------------------------------------------------------------
    // Integration-style tests on log content
    // -----------------------------------------------------------------------

    fn sample_log() -> &'static str {
        concat!(
            "type=SYSCALL msg=audit(1735689600.000:1): arch=c000003e syscall=open success=yes pid=100 uid=0 auid=1000 exe=\"/bin/cat\" key=\"file_access\"\n",
            "type=PATH msg=audit(1735689600.000:1): item=0 name=\"/etc/passwd\" nametype=NORMAL\n",
            "type=CWD msg=audit(1735689600.000:1): cwd=\"/home/user\"\n",
            "type=SYSCALL msg=audit(1735689700.000:2): arch=c000003e syscall=write success=no pid=200 uid=1000 auid=1000 exe=\"/usr/bin/vim\" key=\"write_fail\"\n",
            "type=USER_AUTH msg=audit(1735689800.000:3): pid=300 uid=0 auid=1000 acct=\"admin\" hostname=localhost terminal=ssh res=success\n",
            "type=USER_AUTH msg=audit(1735689900.000:4): pid=301 uid=0 auid=1000 acct=\"hacker\" hostname=evil.com terminal=ssh res=failed\n",
            "type=USER_LOGIN msg=audit(1735690000.000:5): pid=302 uid=0 auid=1000 acct=\"admin\" hostname=localhost terminal=pts/0 res=success\n",
            "type=SYSCALL msg=audit(1735690100.000:6): arch=c000003e syscall=mount success=yes pid=400 uid=0 auid=0 exe=\"/bin/mount\" key=\"mounts\"\n",
            "type=DAEMON_START msg=audit(1735690200.000:7): op=start ver=0.1.0 res=success\n",
            "type=SERVICE_START msg=audit(1735690300.000:8): pid=500 uid=0 auid=4294967295 exe=\"/usr/sbin/sshd\" res=success\n",
        )
    }

    #[test]
    fn test_sample_log_parse_count() {
        let records = load_audit_log_from_str(sample_log());
        assert_eq!(records.len(), 10);
    }

    #[test]
    fn test_sample_log_search_by_key() {
        let records = load_audit_log_from_str(sample_log());
        let criteria = SearchCriteria {
            key: Some("file_access".into()),
            ..Default::default()
        };
        let matches: Vec<_> = records.iter().filter(|r| record_matches(r, &criteria)).collect();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].serial, 1);
    }

    #[test]
    fn test_sample_log_search_by_syscall() {
        let records = load_audit_log_from_str(sample_log());
        let criteria = SearchCriteria {
            syscall: Some("open".into()),
            ..Default::default()
        };
        let matches: Vec<_> = records.iter().filter(|r| record_matches(r, &criteria)).collect();
        assert_eq!(matches.len(), 1);
    }

    #[test]
    fn test_sample_log_search_failures() {
        let records = load_audit_log_from_str(sample_log());
        let criteria = SearchCriteria {
            success: Some(false),
            ..Default::default()
        };
        let matches: Vec<_> = records.iter().filter(|r| record_matches(r, &criteria)).collect();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].syscall_name(), Some("write"));
    }

    #[test]
    fn test_sample_log_search_by_user() {
        let records = load_audit_log_from_str(sample_log());
        let criteria = SearchCriteria {
            user: Some("admin".into()),
            ..Default::default()
        };
        let matches: Vec<_> = records.iter().filter(|r| record_matches(r, &criteria)).collect();
        assert_eq!(matches.len(), 2); // USER_AUTH + USER_LOGIN for admin
    }

    #[test]
    fn test_sample_log_search_by_time() {
        let records = load_audit_log_from_str(sample_log());
        let criteria = SearchCriteria {
            start_time: Some(1_735_689_750.0),
            end_time: Some(1_735_689_950.0),
            ..Default::default()
        };
        let matches: Vec<_> = records.iter().filter(|r| record_matches(r, &criteria)).collect();
        assert_eq!(matches.len(), 2); // auth records at 800 and 900
    }

    #[test]
    fn test_sample_log_search_by_message_type() {
        let records = load_audit_log_from_str(sample_log());
        let criteria = SearchCriteria {
            message_type: Some(MessageType::UserAuth),
            ..Default::default()
        };
        let matches: Vec<_> = records.iter().filter(|r| record_matches(r, &criteria)).collect();
        assert_eq!(matches.len(), 2);
    }

    #[test]
    fn test_sample_log_auth_records() {
        let records = load_audit_log_from_str(sample_log());
        let auth: Vec<_> = records.iter().filter(|r| r.msg_type.is_auth_related()).collect();
        assert_eq!(auth.len(), 2);
    }

    #[test]
    fn test_sample_log_login_records() {
        let records = load_audit_log_from_str(sample_log());
        let login: Vec<_> = records.iter().filter(|r| r.msg_type.is_login_related()).collect();
        assert_eq!(login.len(), 1);
    }

    #[test]
    fn test_sample_log_syscall_records() {
        let records = load_audit_log_from_str(sample_log());
        let syscalls: Vec<_> = records.iter().filter(|r| r.msg_type == MessageType::Syscall).collect();
        assert_eq!(syscalls.len(), 3);
    }

    #[test]
    fn test_sample_log_path_records() {
        let records = load_audit_log_from_str(sample_log());
        let paths: Vec<_> = records.iter().filter(|r| r.msg_type == MessageType::Path).collect();
        assert_eq!(paths.len(), 1);
    }

    #[test]
    fn test_sample_log_combined_search() {
        let records = load_audit_log_from_str(sample_log());
        let criteria = SearchCriteria {
            message_type: Some(MessageType::Syscall),
            success: Some(true),
            ..Default::default()
        };
        let matches: Vec<_> = records.iter().filter(|r| record_matches(r, &criteria)).collect();
        assert_eq!(matches.len(), 2); // open+mount succeeded, write failed
    }
}
