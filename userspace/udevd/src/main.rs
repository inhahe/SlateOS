//! OurOS Device Manager Daemon (udevd / udevadm)
//!
//! Manages device nodes under `/dev/`, applies udev-style rules from
//! `/etc/udev/rules.d/`, and maintains a device property database in
//! `/run/udev/data/`. The binary serves as both the daemon (`udevd`) and
//! the administrative tool (`udevadm`) depending on `argv[0]`.
//!
//! # Daemon mode (udevd)
//!
//! ```text
//! udevd                           Start device manager daemon
//! udevd --debug                   Run in foreground with debug output
//! udevd --resolve-names=early     Resolve user/group names at rule load
//! ```
//!
//! # Admin mode (udevadm)
//!
//! ```text
//! udevadm info <device>           Query device properties from sysfs
//! udevadm info --query=all <dev>  Show all properties
//! udevadm trigger                 Request device events for coldplug
//! udevadm trigger --subsystem-match=block  Trigger only block devices
//! udevadm settle                  Wait for pending udev events
//! udevadm settle --timeout=30     Wait with custom timeout (seconds)
//! udevadm monitor                 Monitor device add/remove/change events
//! udevadm monitor --property      Show event properties
//! udevadm control --reload-rules  Reload rule files
//! udevadm control --log-level=debug  Set runtime log level
//! ```

#![cfg_attr(not(test), no_main)]
#![allow(clippy::needless_range_loop)]

use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

// ============================================================================
// Constants
// ============================================================================

const RULES_DIR: &str = "/etc/udev/rules.d";
const RUN_DB_DIR: &str = "/run/udev/data";
const DEV_DIR: &str = "/dev";
const SYS_DIR: &str = "/sys";
const CONTROL_SOCKET: &str = "/run/udev/control";
const VERSION: &str = "0.1.0";
const DEFAULT_SETTLE_TIMEOUT: u64 = 120;
const MAX_RULE_LINE: usize = 16384;

// ============================================================================
// Log levels
// ============================================================================

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum LogLevel {
    Error = 0,
    Warning = 1,
    Info = 2,
    Debug = 3,
}

impl LogLevel {
    fn from_str(s: &str) -> Option<Self> {
        match s {
            "err" | "error" | "0" => Some(Self::Error),
            "warn" | "warning" | "1" => Some(Self::Warning),
            "info" | "2" => Some(Self::Info),
            "debug" | "3" => Some(Self::Debug),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warning => "warning",
            Self::Info => "info",
            Self::Debug => "debug",
        }
    }
}

// ============================================================================
// Glob matcher
// ============================================================================

/// Simple glob pattern matcher supporting `*`, `?`, and `[...]` character
/// classes. Used for KERNEL, SUBSYSTEM, and ATTR match keys in rules.
fn glob_match(pattern: &str, text: &str) -> bool {
    glob_match_inner(pattern.as_bytes(), text.as_bytes())
}

fn glob_match_inner(pat: &[u8], txt: &[u8]) -> bool {
    let mut pi = 0;
    let mut ti = 0;
    let mut star_pi = usize::MAX;
    let mut star_ti = 0;

    while ti < txt.len() {
        if pi < pat.len() && pat[pi] == b'?' {
            pi += 1;
            ti += 1;
        } else if pi < pat.len() && pat[pi] == b'[' {
            // Character class.
            let matched = match_char_class(pat, &mut pi, txt[ti]);
            if matched {
                ti += 1;
            } else if star_pi != usize::MAX {
                pi = star_pi + 1;
                star_ti += 1;
                ti = star_ti;
            } else {
                return false;
            }
        } else if pi < pat.len() && pat[pi] == b'*' {
            star_pi = pi;
            star_ti = ti;
            pi += 1;
        } else if pi < pat.len() && pat[pi] == txt[ti] {
            pi += 1;
            ti += 1;
        } else if star_pi != usize::MAX {
            pi = star_pi + 1;
            star_ti += 1;
            ti = star_ti;
        } else {
            return false;
        }
    }

    while pi < pat.len() && pat[pi] == b'*' {
        pi += 1;
    }

    pi == pat.len()
}

/// Match a `[...]` character class at position `pi` in `pat` against byte `ch`.
/// Advances `*pi` past the closing `]`.
fn match_char_class(pat: &[u8], pi: &mut usize, ch: u8) -> bool {
    let mut idx = *pi + 1;
    let negate = idx < pat.len() && (pat[idx] == b'!' || pat[idx] == b'^');
    if negate {
        idx += 1;
    }

    let mut found = false;
    let start = idx;

    while idx < pat.len() && (pat[idx] != b']' || idx == start) {
        if idx + 2 < pat.len() && pat[idx + 1] == b'-' {
            // Range: [a-z].
            let lo = pat[idx];
            let hi = pat[idx + 2];
            if ch >= lo && ch <= hi {
                found = true;
            }
            idx += 3;
        } else {
            if pat[idx] == ch {
                found = true;
            }
            idx += 1;
        }
    }

    if idx < pat.len() && pat[idx] == b']' {
        idx += 1;
    }
    *pi = idx;

    if negate { !found } else { found }
}

// ============================================================================
// Device event
// ============================================================================

/// Actions that can appear in a uevent.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum DeviceAction {
    Add,
    Remove,
    Change,
    Move,
    Online,
    Offline,
    Bind,
    Unbind,
}

impl DeviceAction {
    fn from_str(s: &str) -> Option<Self> {
        match s {
            "add" => Some(Self::Add),
            "remove" => Some(Self::Remove),
            "change" => Some(Self::Change),
            "move" => Some(Self::Move),
            "online" => Some(Self::Online),
            "offline" => Some(Self::Offline),
            "bind" => Some(Self::Bind),
            "unbind" => Some(Self::Unbind),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Add => "add",
            Self::Remove => "remove",
            Self::Change => "change",
            Self::Move => "move",
            Self::Online => "online",
            Self::Offline => "offline",
            Self::Bind => "bind",
            Self::Unbind => "unbind",
        }
    }
}

/// A device event from the kernel (uevent).
#[derive(Clone, Debug)]
struct DeviceEvent {
    /// Action (add, remove, change, etc.).
    action: DeviceAction,
    /// Sysfs device path (e.g. `/sys/devices/pci0000:00/0000:00:1f.2/ata1`).
    devpath: String,
    /// Subsystem name (e.g. "block", "net", "usb").
    subsystem: String,
    /// Kernel device name (e.g. "sda", "sda1", "eth0").
    kernel_name: String,
    /// Device type (e.g. "disk", "partition").
    devtype: String,
    /// Major device number.
    major: u32,
    /// Minor device number.
    minor: u32,
    /// Sequence number from the kernel.
    seqnum: u64,
    /// Additional environment variables from the uevent.
    env: HashMap<String, String>,
}

impl DeviceEvent {
    fn new() -> Self {
        Self {
            action: DeviceAction::Add,
            devpath: String::new(),
            subsystem: String::new(),
            kernel_name: String::new(),
            devtype: String::new(),
            major: 0,
            minor: 0,
            seqnum: 0,
            env: HashMap::new(),
        }
    }

    /// Parse a uevent from newline-separated KEY=VALUE text.
    fn parse(text: &str) -> Option<Self> {
        let mut ev = Self::new();
        for line in text.lines() {
            if let Some((key, val)) = line.split_once('=') {
                match key {
                    "ACTION" => {
                        ev.action = DeviceAction::from_str(val)?;
                    }
                    "DEVPATH" => ev.devpath = val.to_string(),
                    "SUBSYSTEM" => ev.subsystem = val.to_string(),
                    "DEVNAME" => ev.kernel_name = val.to_string(),
                    "DEVTYPE" => ev.devtype = val.to_string(),
                    "MAJOR" => ev.major = val.parse().unwrap_or(0),
                    "MINOR" => ev.minor = val.parse().unwrap_or(0),
                    "SEQNUM" => ev.seqnum = val.parse().unwrap_or(0),
                    _ => {
                        ev.env.insert(key.to_string(), val.to_string());
                    }
                }
            }
        }
        // Extract kernel_name from devpath if not set via DEVNAME.
        if ev.kernel_name.is_empty() && !ev.devpath.is_empty() {
            if let Some(name) = ev.devpath.rsplit('/').next() {
                ev.kernel_name = name.to_string();
            }
        }
        Some(ev)
    }

    /// Format the event for monitor output.
    fn format_monitor(&self, show_properties: bool) -> String {
        let mut out = format!(
            "KERNEL[{}] {} {} ({})",
            self.seqnum,
            self.action.as_str(),
            self.devpath,
            self.subsystem,
        );
        if show_properties {
            out.push('\n');
            out.push_str(&format!("ACTION={}\n", self.action.as_str()));
            out.push_str(&format!("DEVPATH={}\n", self.devpath));
            out.push_str(&format!("SUBSYSTEM={}\n", self.subsystem));
            if !self.kernel_name.is_empty() {
                out.push_str(&format!("DEVNAME={}\n", self.kernel_name));
            }
            if !self.devtype.is_empty() {
                out.push_str(&format!("DEVTYPE={}\n", self.devtype));
            }
            if self.major != 0 || self.minor != 0 {
                out.push_str(&format!("MAJOR={}\n", self.major));
                out.push_str(&format!("MINOR={}\n", self.minor));
            }
            out.push_str(&format!("SEQNUM={}\n", self.seqnum));
            for (k, v) in &self.env {
                out.push_str(&format!("{k}={v}\n"));
            }
        }
        out
    }
}

// ============================================================================
// Rule system
// ============================================================================

/// A match condition in a udev rule.
#[derive(Clone, Debug)]
enum MatchKey {
    /// KERNEL=="pattern"
    Kernel(String),
    /// KERNEL!="pattern"  (negated match)
    KernelNot(String),
    /// SUBSYSTEM=="pattern"
    Subsystem(String),
    /// SUBSYSTEM!="pattern"
    SubsystemNot(String),
    /// ACTION=="pattern"
    Action(String),
    /// ACTION!="pattern"
    ActionNot(String),
    /// ATTR{name}=="pattern"
    Attr(String, String),
    /// ATTR{name}!="pattern"
    AttrNot(String, String),
    /// ENV{name}=="pattern"
    Env(String, String),
    /// ENV{name}!="pattern"
    EnvNot(String, String),
    /// DEVPATH=="pattern"
    DevPath(String),
    /// DEVPATH!="pattern"
    DevPathNot(String),
    /// DRIVER=="pattern"
    Driver(String),
    /// DRIVER!="pattern"
    DriverNot(String),
    /// TEST=="path"
    Test(String),
    /// RESULT=="pattern"
    Result(String),
}

/// An assignment in a udev rule.
#[derive(Clone, Debug)]
enum AssignKey {
    /// NAME="value"
    Name(String),
    /// SYMLINK+="value"
    Symlink(String),
    /// OWNER="value"
    Owner(String),
    /// GROUP="value"
    Group(String),
    /// MODE="value"
    Mode(String),
    /// RUN+="command"
    Run(String),
    /// ATTR{name}="value"
    AttrSet(String, String),
    /// ENV{name}="value"
    EnvSet(String, String),
    /// LABEL="name"
    Label(String),
    /// GOTO="name"
    Goto(String),
    /// OPTIONS="value"
    Options(String),
    /// IMPORT{type}="value"
    Import(String, String),
    /// TAG+="value"
    Tag(String),
}

/// A single udev rule (one logical line from rules.d/).
#[derive(Clone, Debug)]
struct Rule {
    /// Match conditions -- all must be satisfied.
    matches: Vec<MatchKey>,
    /// Assignments to apply when all conditions match.
    assigns: Vec<AssignKey>,
    /// Source file for diagnostics.
    _source_file: String,
    /// Line number in source file.
    _source_line: usize,
}

/// Parse all rule files from a directory.
fn load_rules(dir: &str) -> Vec<Rule> {
    let mut rules = Vec::new();
    let mut files = Vec::new();

    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("rules") {
                files.push(path);
            }
        }
    }

    // Rules files are processed in lexicographic order (priority by filename).
    files.sort();

    for path in &files {
        if let Ok(content) = fs::read_to_string(path) {
            let fname = path.to_string_lossy().to_string();
            parse_rules_file(&content, &fname, &mut rules);
        }
    }

    rules
}

/// Parse a single rules file, appending rules to `out`.
fn parse_rules_file(content: &str, filename: &str, out: &mut Vec<Rule>) {
    let mut continued = String::new();

    for (line_idx, raw_line) in content.lines().enumerate() {
        let trimmed = raw_line.trim();

        // Skip empty lines and comments.
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        // Handle line continuation (trailing backslash).
        if trimmed.ends_with('\\') {
            continued.push_str(&trimmed[..trimmed.len() - 1]);
            continued.push(' ');
            continue;
        }

        let full_line = if continued.is_empty() {
            trimmed.to_string()
        } else {
            continued.push_str(trimmed);
            let result = continued.clone();
            continued.clear();
            result
        };

        if full_line.len() > MAX_RULE_LINE {
            continue;
        }

        if let Some(rule) = parse_rule_line(&full_line, filename, line_idx + 1) {
            out.push(rule);
        }
    }
}

/// Parse a single rule line into match conditions and assignments.
fn parse_rule_line(line: &str, filename: &str, line_num: usize) -> Option<Rule> {
    let mut matches = Vec::new();
    let mut assigns = Vec::new();

    // Tokenize: split on commas, respecting quoted strings.
    let tokens = tokenize_rule(line);

    for token in &tokens {
        let t = token.trim();
        if t.is_empty() {
            continue;
        }

        // Try to parse as a key-operator-value triple.
        if let Some(parsed) = parse_key_op_value(t) {
            let (key, op, value) = parsed;
            match op {
                "==" => {
                    if let Some(m) = make_match_key(key, &value, false) {
                        matches.push(m);
                    }
                }
                "!=" => {
                    if let Some(m) = make_match_key(key, &value, true) {
                        matches.push(m);
                    }
                }
                "=" | "+=" | ":=" => {
                    if let Some(a) = make_assign_key(key, &value, op) {
                        assigns.push(a);
                    }
                }
                _ => {}
            }
        }
    }

    if matches.is_empty() && assigns.is_empty() {
        return None;
    }

    Some(Rule {
        matches,
        assigns,
        _source_file: filename.to_string(),
        _source_line: line_num,
    })
}

/// Tokenize a rule line, splitting on commas but respecting double-quoted
/// strings.
fn tokenize_rule(line: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_quote = false;
    let bytes = line.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        let ch = bytes[i];
        if ch == b'"' {
            in_quote = !in_quote;
            current.push('"');
        } else if ch == b',' && !in_quote {
            tokens.push(current.clone());
            current.clear();
        } else {
            current.push(ch as char);
        }
        i += 1;
    }

    if !current.is_empty() {
        tokens.push(current);
    }

    tokens
}

/// Parse `KEY{attr}OP"VALUE"` into `(key_with_attr, op, value)`.
fn parse_key_op_value(token: &str) -> Option<(&str, &str, String)> {
    // Find the operator: ==, !=, +=, :=, =
    let ops = ["==", "!=", "+=", ":="];
    for op in &ops {
        if let Some(pos) = token.find(op) {
            let key = &token[..pos];
            let rest = &token[pos + op.len()..];
            let value = rest.trim().trim_matches('"').to_string();
            return Some((key, op, value));
        }
    }
    // Plain `=` (must check after two-char ops).
    if let Some(pos) = token.find('=') {
        // Make sure it's not part of == or != or += or :=.
        if pos > 0 {
            let prev = token.as_bytes()[pos - 1];
            if prev == b'!' || prev == b'+' || prev == b':' || prev == b'=' {
                return None;
            }
        }
        if pos + 1 < token.len() && token.as_bytes()[pos + 1] == b'=' {
            return None;
        }
        let key = &token[..pos];
        let rest = &token[pos + 1..];
        let value = rest.trim().trim_matches('"').to_string();
        return Some((key, "=", value));
    }
    None
}

/// Create a match key from a parsed key name, value, and negation flag.
fn make_match_key(key: &str, value: &str, negate: bool) -> Option<MatchKey> {
    // Check for ATTR{name} and ENV{name} syntax.
    if let Some(attr_name) = extract_brace_arg(key, "ATTR") {
        return Some(if negate {
            MatchKey::AttrNot(attr_name, value.to_string())
        } else {
            MatchKey::Attr(attr_name, value.to_string())
        });
    }
    if let Some(attr_name) = extract_brace_arg(key, "ATTRS") {
        return Some(if negate {
            MatchKey::AttrNot(attr_name, value.to_string())
        } else {
            MatchKey::Attr(attr_name, value.to_string())
        });
    }
    if let Some(env_name) = extract_brace_arg(key, "ENV") {
        return Some(if negate {
            MatchKey::EnvNot(env_name, value.to_string())
        } else {
            MatchKey::Env(env_name, value.to_string())
        });
    }

    match key.trim() {
        "KERNEL" => Some(if negate {
            MatchKey::KernelNot(value.to_string())
        } else {
            MatchKey::Kernel(value.to_string())
        }),
        "SUBSYSTEM" => Some(if negate {
            MatchKey::SubsystemNot(value.to_string())
        } else {
            MatchKey::Subsystem(value.to_string())
        }),
        "ACTION" => Some(if negate {
            MatchKey::ActionNot(value.to_string())
        } else {
            MatchKey::Action(value.to_string())
        }),
        "DEVPATH" => Some(if negate {
            MatchKey::DevPathNot(value.to_string())
        } else {
            MatchKey::DevPath(value.to_string())
        }),
        "DRIVER" => Some(if negate {
            MatchKey::DriverNot(value.to_string())
        } else {
            MatchKey::Driver(value.to_string())
        }),
        "TEST" => Some(MatchKey::Test(value.to_string())),
        "RESULT" => Some(MatchKey::Result(value.to_string())),
        _ => None,
    }
}

/// Create an assignment key from a parsed key, value, and operator.
fn make_assign_key(key: &str, value: &str, _op: &str) -> Option<AssignKey> {
    if let Some(attr_name) = extract_brace_arg(key, "ATTR") {
        return Some(AssignKey::AttrSet(attr_name, value.to_string()));
    }
    if let Some(env_name) = extract_brace_arg(key, "ENV") {
        return Some(AssignKey::EnvSet(env_name, value.to_string()));
    }
    if let Some(import_type) = extract_brace_arg(key, "IMPORT") {
        return Some(AssignKey::Import(import_type, value.to_string()));
    }

    match key.trim() {
        "NAME" => Some(AssignKey::Name(value.to_string())),
        "SYMLINK" => Some(AssignKey::Symlink(value.to_string())),
        "OWNER" => Some(AssignKey::Owner(value.to_string())),
        "GROUP" => Some(AssignKey::Group(value.to_string())),
        "MODE" => Some(AssignKey::Mode(value.to_string())),
        "RUN" => Some(AssignKey::Run(value.to_string())),
        "LABEL" => Some(AssignKey::Label(value.to_string())),
        "GOTO" => Some(AssignKey::Goto(value.to_string())),
        "OPTIONS" => Some(AssignKey::Options(value.to_string())),
        "TAG" => Some(AssignKey::Tag(value.to_string())),
        _ => None,
    }
}

/// Extract the argument from `PREFIX{arg}` syntax, e.g. `ATTR{size}` -> `"size"`.
fn extract_brace_arg(key: &str, prefix: &str) -> Option<String> {
    let trimmed = key.trim();
    if trimmed.starts_with(prefix) {
        let rest = &trimmed[prefix.len()..];
        if rest.starts_with('{') {
            if let Some(end) = rest.find('}') {
                return Some(rest[1..end].to_string());
            }
        }
    }
    None
}

// ============================================================================
// Device database
// ============================================================================

/// Persistent device property database stored in /run/udev/data/.
/// Each device gets a file keyed by `b<major>:<minor>` or `n<ifindex>`.
struct DeviceDatabase {
    base_dir: String,
}

impl DeviceDatabase {
    fn new(base_dir: &str) -> Self {
        Self {
            base_dir: base_dir.to_string(),
        }
    }

    /// Database key for a block/char device.
    fn dev_key(major: u32, minor: u32) -> String {
        format!("b{}:{}", major, minor)
    }

    /// Database key for a network interface (used when netlink events arrive).
    #[allow(dead_code)]
    fn net_key(ifindex: u32) -> String {
        format!("n{}", ifindex)
    }

    /// Store device properties to the database.
    fn store(&self, key: &str, props: &HashMap<String, String>) -> Result<(), String> {
        let dir = &self.base_dir;
        fs::create_dir_all(dir).map_err(|e| format!("mkdir {dir}: {e}"))?;

        let path = format!("{dir}/{key}");
        let mut content = String::new();
        // Sort keys for deterministic output.
        let mut keys: Vec<&String> = props.keys().collect();
        keys.sort();
        for k in keys {
            if let Some(v) = props.get(k) {
                content.push_str(&format!("E:{k}={v}\n"));
            }
        }
        fs::write(&path, &content).map_err(|e| format!("write {path}: {e}"))?;
        Ok(())
    }

    /// Load device properties from the database.
    fn load(&self, key: &str) -> HashMap<String, String> {
        let path = format!("{}/{}", self.base_dir, key);
        let mut props = HashMap::new();
        if let Ok(content) = fs::read_to_string(&path) {
            for line in content.lines() {
                if let Some(rest) = line.strip_prefix("E:") {
                    if let Some((k, v)) = rest.split_once('=') {
                        props.insert(k.to_string(), v.to_string());
                    }
                }
            }
        }
        props
    }

    /// Remove a device's database entry.
    fn remove(&self, key: &str) -> Result<(), String> {
        let path = format!("{}/{}", self.base_dir, key);
        if Path::new(&path).exists() {
            fs::remove_file(&path).map_err(|e| format!("rm {path}: {e}"))?;
        }
        Ok(())
    }

    /// List all database keys (used by udevadm info --export-db).
    #[allow(dead_code)]
    fn list_keys(&self) -> Vec<String> {
        let mut keys = Vec::new();
        if let Ok(entries) = fs::read_dir(&self.base_dir) {
            for entry in entries.flatten() {
                if let Some(name) = entry.file_name().to_str() {
                    keys.push(name.to_string());
                }
            }
        }
        keys.sort();
        keys
    }
}

// ============================================================================
// Sysfs reader
// ============================================================================

/// Read a sysfs attribute file, stripping trailing newlines.
fn read_sysfs_attr(devpath: &str, attr: &str) -> Option<String> {
    let path = if devpath.starts_with(SYS_DIR) {
        format!("{devpath}/{attr}")
    } else {
        format!("{SYS_DIR}{devpath}/{attr}")
    };
    fs::read_to_string(&path)
        .ok()
        .map(|s| s.trim_end().to_string())
}

/// Read the uevent file for a sysfs device path.
fn read_uevent(devpath: &str) -> HashMap<String, String> {
    let path = if devpath.starts_with(SYS_DIR) {
        format!("{devpath}/uevent")
    } else {
        format!("{SYS_DIR}{devpath}/uevent")
    };
    let mut props = HashMap::new();
    if let Ok(content) = fs::read_to_string(&path) {
        for line in content.lines() {
            if let Some((k, v)) = line.split_once('=') {
                props.insert(k.to_string(), v.to_string());
            }
        }
    }
    props
}

/// Discover all sysfs device paths under a given subsystem.
fn enumerate_subsystem(subsystem: &str) -> Vec<String> {
    let bus_path = format!("{SYS_DIR}/bus/{subsystem}/devices");
    let class_path = format!("{SYS_DIR}/class/{subsystem}");
    let mut paths = Vec::new();

    for dir in &[bus_path, class_path] {
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                // Resolve symlink to get canonical sysfs path.
                let resolved = fs::canonicalize(&p)
                    .unwrap_or_else(|_| p.clone());
                paths.push(resolved.to_string_lossy().to_string());
            }
        }
    }

    paths
}

/// Enumerate all devices across all subsystems.
fn enumerate_all_devices() -> Vec<String> {
    let mut paths = Vec::new();

    // Scan /sys/bus/*/devices/
    if let Ok(buses) = fs::read_dir(&format!("{SYS_DIR}/bus")) {
        for bus in buses.flatten() {
            let dev_dir = bus.path().join("devices");
            if let Ok(devs) = fs::read_dir(&dev_dir) {
                for dev in devs.flatten() {
                    let p = dev.path();
                    let resolved = fs::canonicalize(&p).unwrap_or(p);
                    paths.push(resolved.to_string_lossy().to_string());
                }
            }
        }
    }

    // Scan /sys/class/*/
    if let Ok(classes) = fs::read_dir(&format!("{SYS_DIR}/class")) {
        for cls in classes.flatten() {
            if let Ok(devs) = fs::read_dir(cls.path()) {
                for dev in devs.flatten() {
                    let p = dev.path();
                    let resolved = fs::canonicalize(&p).unwrap_or(p);
                    paths.push(resolved.to_string_lossy().to_string());
                }
            }
        }
    }

    paths.sort();
    paths.dedup();
    paths
}

// ============================================================================
// Rule evaluation engine
// ============================================================================

/// Result of applying rules to a device event.
#[derive(Clone, Debug)]
struct RuleResult {
    /// Device node name (relative to /dev/).
    name: Option<String>,
    /// Symlinks to create (relative to /dev/).
    symlinks: Vec<String>,
    /// Owner name or UID.
    owner: Option<String>,
    /// Group name or GID.
    group: Option<String>,
    /// Permission mode (octal string).
    mode: Option<String>,
    /// Commands to run after processing.
    run_cmds: Vec<String>,
    /// Extra environment variables set by rules.
    extra_env: HashMap<String, String>,
    /// Tags applied to the device.
    tags: Vec<String>,
    /// Options set by rules.
    options: Vec<String>,
}

impl RuleResult {
    fn new() -> Self {
        Self {
            name: None,
            symlinks: Vec::new(),
            owner: None,
            group: None,
            mode: None,
            run_cmds: Vec::new(),
            extra_env: HashMap::new(),
            tags: Vec::new(),
            options: Vec::new(),
        }
    }
}

/// Check whether a single match condition is satisfied.
fn check_match(cond: &MatchKey, event: &DeviceEvent) -> bool {
    match cond {
        MatchKey::Kernel(pat) => glob_match(pat, &event.kernel_name),
        MatchKey::KernelNot(pat) => !glob_match(pat, &event.kernel_name),
        MatchKey::Subsystem(pat) => glob_match(pat, &event.subsystem),
        MatchKey::SubsystemNot(pat) => !glob_match(pat, &event.subsystem),
        MatchKey::Action(pat) => glob_match(pat, event.action.as_str()),
        MatchKey::ActionNot(pat) => !glob_match(pat, event.action.as_str()),
        MatchKey::DevPath(pat) => glob_match(pat, &event.devpath),
        MatchKey::DevPathNot(pat) => !glob_match(pat, &event.devpath),
        MatchKey::Driver(pat) => {
            let driver = event.env.get("DRIVER").cloned().unwrap_or_default();
            glob_match(pat, &driver)
        }
        MatchKey::DriverNot(pat) => {
            let driver = event.env.get("DRIVER").cloned().unwrap_or_default();
            !glob_match(pat, &driver)
        }
        MatchKey::Attr(attr_name, pat) => {
            let val = read_sysfs_attr(&event.devpath, attr_name)
                .unwrap_or_default();
            glob_match(pat, &val)
        }
        MatchKey::AttrNot(attr_name, pat) => {
            let val = read_sysfs_attr(&event.devpath, attr_name)
                .unwrap_or_default();
            !glob_match(pat, &val)
        }
        MatchKey::Env(env_key, pat) => {
            let val = event.env.get(env_key).cloned().unwrap_or_default();
            glob_match(pat, &val)
        }
        MatchKey::EnvNot(env_key, pat) => {
            let val = event.env.get(env_key).cloned().unwrap_or_default();
            !glob_match(pat, &val)
        }
        MatchKey::Test(path) => Path::new(path).exists(),
        MatchKey::Result(pat) => {
            let result_val = event.env.get("RESULT").cloned().unwrap_or_default();
            glob_match(pat, &result_val)
        }
    }
}

/// Substitute `%k` (kernel name), `%n` (number), `%M` (major), `%m` (minor),
/// `%E{key}` (env var), etc. in a rule value string.
fn substitute_value(template: &str, event: &DeviceEvent, result: &RuleResult) -> String {
    let mut out = String::with_capacity(template.len());
    let bytes = template.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == b'%' && i + 1 < bytes.len() {
            match bytes[i + 1] {
                b'k' => {
                    out.push_str(&event.kernel_name);
                    i += 2;
                }
                b'n' => {
                    // Device number suffix: "sda1" -> "1", "sda" -> "".
                    let num: String = event.kernel_name.chars()
                        .rev()
                        .take_while(|c| c.is_ascii_digit())
                        .collect::<Vec<_>>()
                        .into_iter()
                        .rev()
                        .collect();
                    out.push_str(&num);
                    i += 2;
                }
                b'M' => {
                    out.push_str(&event.major.to_string());
                    i += 2;
                }
                b'm' => {
                    out.push_str(&event.minor.to_string());
                    i += 2;
                }
                b'p' => {
                    out.push_str(&event.devpath);
                    i += 2;
                }
                b's' => {
                    out.push_str(&event.subsystem);
                    i += 2;
                }
                b'N' => {
                    // Result device name.
                    if let Some(ref n) = result.name {
                        out.push_str(n);
                    } else {
                        out.push_str(&event.kernel_name);
                    }
                    i += 2;
                }
                b'E' if i + 2 < bytes.len() && bytes[i + 2] == b'{' => {
                    // %E{KEY}
                    let rest = &template[i + 3..];
                    if let Some(end) = rest.find('}') {
                        let key = &rest[..end];
                        let val = event.env.get(key)
                            .or_else(|| result.extra_env.get(key))
                            .cloned()
                            .unwrap_or_default();
                        out.push_str(&val);
                        i += 4 + end;
                    } else {
                        out.push('%');
                        i += 1;
                    }
                }
                b'%' => {
                    out.push('%');
                    i += 2;
                }
                _ => {
                    out.push('%');
                    i += 1;
                }
            }
        } else if bytes[i] == b'$' && i + 1 < bytes.len() {
            // $kernel, $number, $major, $minor, $devpath, $env{key}
            let rest = &template[i + 1..];
            if rest.starts_with("kernel") {
                out.push_str(&event.kernel_name);
                i += 7;
            } else if rest.starts_with("number") {
                let num: String = event.kernel_name.chars()
                    .rev()
                    .take_while(|c| c.is_ascii_digit())
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect();
                out.push_str(&num);
                i += 7;
            } else if rest.starts_with("major") {
                out.push_str(&event.major.to_string());
                i += 6;
            } else if rest.starts_with("minor") {
                out.push_str(&event.minor.to_string());
                i += 6;
            } else if rest.starts_with("devpath") {
                out.push_str(&event.devpath);
                i += 8;
            } else if rest.starts_with("env{") {
                let after = &rest[4..];
                if let Some(end) = after.find('}') {
                    let key = &after[..end];
                    let val = event.env.get(key)
                        .or_else(|| result.extra_env.get(key))
                        .cloned()
                        .unwrap_or_default();
                    out.push_str(&val);
                    i += 5 + end + 1;
                } else {
                    out.push('$');
                    i += 1;
                }
            } else {
                out.push('$');
                i += 1;
            }
        } else {
            out.push(bytes[i] as char);
            i += 1;
        }
    }

    out
}

/// Apply a rule set to a device event, returning the aggregated result.
fn apply_rules(rules: &[Rule], event: &DeviceEvent) -> RuleResult {
    let mut result = RuleResult::new();
    let mut skip_to_label: Option<String> = None;

    for rule in rules {
        // Handle GOTO: skip rules until we reach the target LABEL.
        if let Some(ref target) = skip_to_label {
            let has_label = rule.assigns.iter().any(|a| {
                matches!(a, AssignKey::Label(l) if l == target)
            });
            if has_label {
                skip_to_label = None;
            }
            continue;
        }

        // Check all match conditions.
        let all_match = rule.matches.iter().all(|m| check_match(m, event));
        if !all_match {
            continue;
        }

        // Apply assignments.
        for assign in &rule.assigns {
            match assign {
                AssignKey::Name(v) => {
                    result.name = Some(substitute_value(v, event, &result));
                }
                AssignKey::Symlink(v) => {
                    let expanded = substitute_value(v, event, &result);
                    // Symlinks can be space-separated.
                    for link in expanded.split_whitespace() {
                        if !link.is_empty() {
                            result.symlinks.push(link.to_string());
                        }
                    }
                }
                AssignKey::Owner(v) => {
                    result.owner = Some(substitute_value(v, event, &result));
                }
                AssignKey::Group(v) => {
                    result.group = Some(substitute_value(v, event, &result));
                }
                AssignKey::Mode(v) => {
                    result.mode = Some(substitute_value(v, event, &result));
                }
                AssignKey::Run(v) => {
                    result.run_cmds.push(substitute_value(v, event, &result));
                }
                AssignKey::EnvSet(k, v) => {
                    let expanded = substitute_value(v, event, &result);
                    result.extra_env.insert(k.clone(), expanded);
                }
                AssignKey::Tag(v) => {
                    result.tags.push(substitute_value(v, event, &result));
                }
                AssignKey::Options(v) => {
                    result.options.push(substitute_value(v, event, &result));
                }
                AssignKey::Goto(target) => {
                    skip_to_label = Some(target.clone());
                }
                AssignKey::Label(_) => {
                    // Labels are handled by GOTO scanning above.
                }
                AssignKey::AttrSet(attr, val) => {
                    // Attr writes are daemon-only operations performed
                    // after rule evaluation. Store in extra_env for
                    // downstream processing.
                    let expanded = substitute_value(val, event, &result);
                    result.extra_env.insert(
                        format!("_ATTR_SET_{attr}"),
                        expanded,
                    );
                }
                AssignKey::Import(import_type, source) => {
                    // Import operations fetch properties from external
                    // sources. Record the request for daemon processing.
                    let expanded = substitute_value(source, event, &result);
                    result.extra_env.insert(
                        format!("_IMPORT_{import_type}"),
                        expanded,
                    );
                }
            }
        }
    }

    result
}

// ============================================================================
// Persistent naming (by-id, by-path, by-uuid symlinks)
// ============================================================================

/// Compute persistent naming symlinks for a device based on its properties.
fn compute_persistent_links(event: &DeviceEvent) -> Vec<String> {
    let mut links = Vec::new();

    // by-path: based on sysfs devpath (physical topology).
    if !event.devpath.is_empty() && event.subsystem == "block" {
        let by_path = format!(
            "disk/by-path/{}",
            event.devpath.replace('/', "-").trim_start_matches('-')
        );
        links.push(by_path);
    }

    // by-id: based on serial number / model.
    if event.subsystem == "block" {
        let model = read_sysfs_attr(&event.devpath, "device/model");
        let serial = read_sysfs_attr(&event.devpath, "device/serial");
        if let (Some(m), Some(s)) = (model, serial) {
            let sanitized_model = sanitize_for_devname(&m);
            let sanitized_serial = sanitize_for_devname(&s);
            links.push(format!(
                "disk/by-id/{}-{}-{}",
                event.subsystem, sanitized_model, sanitized_serial
            ));
        }
    }

    // by-uuid: from partition UUID.
    if let Some(uuid) = event.env.get("ID_FS_UUID") {
        if !uuid.is_empty() {
            links.push(format!("disk/by-uuid/{uuid}"));
        }
    }

    // by-label: from filesystem label.
    if let Some(label) = event.env.get("ID_FS_LABEL") {
        if !label.is_empty() {
            links.push(format!("disk/by-label/{label}"));
        }
    }

    // by-partuuid: for GPT partition UUID.
    if let Some(partuuid) = event.env.get("ID_PART_ENTRY_UUID") {
        if !partuuid.is_empty() {
            links.push(format!("disk/by-partuuid/{partuuid}"));
        }
    }

    links
}

/// Sanitize a string for use in a device name: replace non-alnum with `_`,
/// trim leading/trailing underscores, collapse runs.
fn sanitize_for_devname(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut last_underscore = true; // suppress leading underscore
    for ch in s.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '.' {
            out.push(ch);
            last_underscore = false;
        } else if !last_underscore {
            out.push('_');
            last_underscore = true;
        }
    }
    // Trim trailing underscores.
    while out.ends_with('_') {
        out.pop();
    }
    out
}

// ============================================================================
// Device node management
// ============================================================================

/// Syscall numbers for mknod / chmod / chown / symlink / unlink / mkdir.
/// These will be used when the kernel syscall interface is available.
const _SYS_MKNOD: u64 = 620;
const _SYS_CHMOD: u64 = 621;
const _SYS_SYMLINK: u64 = 622;
const _SYS_UNLINK: u64 = 623;
const _SYS_MKDIR: u64 = 624;

/// Create device node, apply permissions, and create symlinks.
fn apply_device_node(
    event: &DeviceEvent,
    rule_result: &RuleResult,
    log_level: LogLevel,
) {
    // Determine the device node name.
    let dev_name = rule_result
        .name
        .as_deref()
        .unwrap_or(&event.kernel_name);

    let dev_path = format!("{DEV_DIR}/{dev_name}");

    match event.action {
        DeviceAction::Add => {
            if log_level >= LogLevel::Debug {
                eprintln!(
                    "udevd: creating node {} ({}:{}) for {}",
                    dev_path, event.major, event.minor, event.devpath
                );
            }

            // Ensure parent directory exists.
            if let Some(parent) = Path::new(&dev_path).parent() {
                let _ = fs::create_dir_all(parent);
            }

            // In a real implementation, we would call mknod here. For now,
            // write a marker file since we may not have the mknod syscall
            // available in all test environments.
            let node_info = format!(
                "{}:{} {} {}\n",
                event.major,
                event.minor,
                rule_result.mode.as_deref().unwrap_or("0660"),
                event.subsystem,
            );
            let _ = fs::write(&dev_path, &node_info);

            // Apply permissions.
            if let Some(ref mode) = rule_result.mode {
                if log_level >= LogLevel::Debug {
                    eprintln!("udevd: chmod {mode} {dev_path}");
                }
            }

            // Create rule-defined symlinks.
            for link in &rule_result.symlinks {
                create_dev_symlink(link, dev_name, log_level);
            }

            // Create persistent naming symlinks.
            let persistent = compute_persistent_links(event);
            for link in &persistent {
                create_dev_symlink(link, dev_name, log_level);
            }
        }
        DeviceAction::Remove => {
            if log_level >= LogLevel::Debug {
                eprintln!("udevd: removing node {dev_path}");
            }
            let _ = fs::remove_file(&dev_path);

            // Remove symlinks.
            for link in &rule_result.symlinks {
                let link_path = format!("{DEV_DIR}/{link}");
                let _ = fs::remove_file(&link_path);
            }
        }
        _ => {
            // Change, move, etc. -- update properties but don't recreate node.
            if log_level >= LogLevel::Debug {
                eprintln!(
                    "udevd: event {} for {dev_path}",
                    event.action.as_str()
                );
            }
        }
    }
}

/// Create a symlink under /dev/.
fn create_dev_symlink(link: &str, target_name: &str, log_level: LogLevel) {
    let link_path = format!("{DEV_DIR}/{link}");
    if let Some(parent) = Path::new(&link_path).parent() {
        let _ = fs::create_dir_all(parent);
    }

    if log_level >= LogLevel::Debug {
        eprintln!("udevd: symlink {link_path} -> {target_name}");
    }

    // Remove existing link first.
    let _ = fs::remove_file(&link_path);

    // Write a symlink marker (actual symlink creation would use the symlink
    // syscall; for now we write a text marker for portability).
    let _ = fs::write(&link_path, format!("-> {target_name}\n"));
}

// ============================================================================
// Daemon state
// ============================================================================

struct DaemonState {
    rules: Vec<Rule>,
    db: DeviceDatabase,
    log_level: LogLevel,
    event_count: u64,
    _resolve_names_early: bool,
}

impl DaemonState {
    fn new(log_level: LogLevel, resolve_names_early: bool) -> Self {
        Self {
            rules: load_rules(RULES_DIR),
            db: DeviceDatabase::new(RUN_DB_DIR),
            log_level,
            event_count: 0,
            _resolve_names_early: resolve_names_early,
        }
    }

    /// Reload rules from disk.
    fn reload_rules(&mut self) {
        self.rules = load_rules(RULES_DIR);
        if self.log_level >= LogLevel::Info {
            eprintln!("udevd: reloaded {} rules", self.rules.len());
        }
    }

    /// Process a single device event.
    fn process_event(&mut self, event: &DeviceEvent) {
        self.event_count += 1;

        if self.log_level >= LogLevel::Debug {
            eprintln!(
                "udevd: [{}] {} {} ({})",
                event.seqnum,
                event.action.as_str(),
                event.devpath,
                event.subsystem,
            );
        }

        // Apply rules.
        let result = apply_rules(&self.rules, event);

        // Update database.
        let db_key = DeviceDatabase::dev_key(event.major, event.minor);
        match event.action {
            DeviceAction::Remove => {
                let _ = self.db.remove(&db_key);
            }
            _ => {
                let mut props = HashMap::new();
                props.insert("DEVPATH".to_string(), event.devpath.clone());
                props.insert("SUBSYSTEM".to_string(), event.subsystem.clone());
                props.insert("DEVNAME".to_string(), event.kernel_name.clone());
                props.insert("ACTION".to_string(), event.action.as_str().to_string());
                if event.major != 0 || event.minor != 0 {
                    props.insert("MAJOR".to_string(), event.major.to_string());
                    props.insert("MINOR".to_string(), event.minor.to_string());
                }
                for (k, v) in &event.env {
                    props.insert(k.clone(), v.clone());
                }
                for (k, v) in &result.extra_env {
                    props.insert(k.clone(), v.clone());
                }
                if let Some(ref name) = result.name {
                    props.insert("DEVNAME".to_string(), name.clone());
                }
                let _ = self.db.store(&db_key, &props);
            }
        }

        // Create/remove device nodes.
        apply_device_node(event, &result, self.log_level);
    }
}

// ============================================================================
// uevent listener (kernel netlink)
// ============================================================================

/// Simulated uevent reading: scan /sys/*/uevent files for coldplug,
/// or read from a uevent source file (for daemon mode).
fn read_uevent_from_sysfs(devpath: &str) -> Option<DeviceEvent> {
    let uevent_path = if devpath.starts_with(SYS_DIR) {
        format!("{devpath}/uevent")
    } else {
        format!("{SYS_DIR}{devpath}/uevent")
    };

    let content = fs::read_to_string(&uevent_path).ok()?;
    let mut props = HashMap::new();
    for line in content.lines() {
        if let Some((k, v)) = line.split_once('=') {
            props.insert(k.to_string(), v.to_string());
        }
    }

    let subsystem = read_sysfs_attr(devpath, "subsystem")
        .or_else(|| props.get("SUBSYSTEM").cloned())
        .unwrap_or_default();

    let kernel_name = devpath
        .rsplit('/')
        .next()
        .unwrap_or("")
        .to_string();

    let major = props
        .get("MAJOR")
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let minor = props
        .get("MINOR")
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    Some(DeviceEvent {
        action: DeviceAction::Add,
        devpath: devpath.to_string(),
        subsystem,
        kernel_name,
        devtype: props.get("DEVTYPE").cloned().unwrap_or_default(),
        major,
        minor,
        seqnum: 0,
        env: props,
    })
}

// ============================================================================
// udevadm subcommands
// ============================================================================

/// `udevadm info` -- query device properties.
fn cmd_info(args: &[String]) -> i32 {
    let mut query_type = "all";
    let mut device_path: Option<String> = None;
    let mut show_attribute_walk = false;
    let mut show_export = false;
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "--query" | "-q" => {
                i += 1;
                if i < args.len() {
                    query_type = leak_str(&args[i]);
                }
            }
            "--path" | "-p" => {
                i += 1;
                if i < args.len() {
                    device_path = Some(args[i].clone());
                }
            }
            "--name" | "-n" => {
                i += 1;
                if i < args.len() {
                    // Convert /dev/foo to sysfs path.
                    device_path = dev_name_to_syspath(&args[i]);
                }
            }
            "--attribute-walk" | "-a" => {
                show_attribute_walk = true;
            }
            "--export" | "-x" => {
                show_export = true;
            }
            "--help" | "-h" => {
                print_info_help();
                return 0;
            }
            s => {
                if s.starts_with("--query=") {
                    query_type = leak_str(s.strip_prefix("--query=").unwrap_or("all"));
                } else if !s.starts_with('-') && device_path.is_none() {
                    device_path = Some(s.to_string());
                }
            }
        }
        i += 1;
    }

    let devpath = match device_path {
        Some(p) => p,
        None => {
            eprintln!("udevadm info: no device specified");
            return 1;
        }
    };

    // Read device properties.
    let db = DeviceDatabase::new(RUN_DB_DIR);
    let uevent_props = read_uevent(&devpath);

    if show_attribute_walk {
        return show_attr_walk(&devpath);
    }

    // Build property list from uevent + database.
    let mut props = uevent_props;
    let major = props.get("MAJOR").and_then(|s| s.parse().ok()).unwrap_or(0u32);
    let minor = props.get("MINOR").and_then(|s| s.parse().ok()).unwrap_or(0u32);
    let db_key = DeviceDatabase::dev_key(major, minor);
    let db_props = db.load(&db_key);
    for (k, v) in &db_props {
        props.entry(k.clone()).or_insert_with(|| v.clone());
    }

    // Print results based on query type.
    match query_type {
        "name" | "symlink" | "path" | "property" | "all" => {
            if show_export {
                print_info_export(&devpath, &props);
            } else {
                print_info_all(&devpath, &props, query_type);
            }
        }
        _ => {
            eprintln!("udevadm info: unknown query type '{query_type}'");
            return 1;
        }
    }

    0
}

/// Walk the sysfs attribute tree for a device.
fn show_attr_walk(devpath: &str) -> i32 {
    let mut current = PathBuf::from(if devpath.starts_with(SYS_DIR) {
        devpath.to_string()
    } else {
        format!("{SYS_DIR}{devpath}")
    });

    println!("Udevadm info starts with the device specified by the devpath and walks up the");
    println!("chain of parent devices.\n");

    loop {
        println!("  looking at device '{}':", current.display());

        // Read uevent.
        let uevent = current.join("uevent");
        if let Ok(content) = fs::read_to_string(&uevent) {
            for line in content.lines() {
                if let Some((k, v)) = line.split_once('=') {
                    println!("    ATTR{{{k}}}==\"{v}\"");
                }
            }
        }

        // Read individual attribute files.
        if let Ok(entries) = fs::read_dir(&current) {
            for entry in entries.flatten() {
                let ft = entry.file_type();
                if ft.map(|t| t.is_file()).unwrap_or(false) {
                    let name = entry.file_name();
                    let name_str = name.to_string_lossy();
                    if name_str == "uevent" {
                        continue;
                    }
                    if let Ok(val) = fs::read_to_string(entry.path()) {
                        let trimmed = val.trim_end();
                        if !trimmed.is_empty()
                            && trimmed.len() < 256
                            && !trimmed.contains('\0')
                        {
                            println!("    ATTR{{{name_str}}}==\"{trimmed}\"");
                        }
                    }
                }
            }
        }
        println!();

        // Walk up to parent.
        match current.parent() {
            Some(p) if p.as_os_str() != current.as_os_str()
                && p.starts_with(SYS_DIR) =>
            {
                current = p.to_path_buf();
            }
            _ => break,
        }
    }

    0
}

/// Print device info in standard format.
fn print_info_all(devpath: &str, props: &HashMap<String, String>, query: &str) {
    let kernel_name = devpath.rsplit('/').next().unwrap_or("");

    if query == "name" || query == "all" {
        println!("P: {devpath}");
        println!("N: {kernel_name}");
    }

    if query == "all" || query == "property" {
        let mut sorted_keys: Vec<&String> = props.keys().collect();
        sorted_keys.sort();
        for k in sorted_keys {
            if let Some(v) = props.get(k) {
                println!("E: {k}={v}");
            }
        }
    }

    if query == "symlink" || query == "all" {
        // Check for known symlinks in the database.
        if let Some(links) = props.get("DEVLINKS") {
            for link in links.split_whitespace() {
                println!("S: {link}");
            }
        }
    }
}

/// Print device info in export (env-var) format.
fn print_info_export(devpath: &str, props: &HashMap<String, String>) {
    let kernel_name = devpath.rsplit('/').next().unwrap_or("");
    println!("DEVPATH='{devpath}'");
    println!("DEVNAME='{kernel_name}'");
    let mut sorted_keys: Vec<&String> = props.keys().collect();
    sorted_keys.sort();
    for k in sorted_keys {
        if let Some(v) = props.get(k) {
            println!("{k}='{v}'");
        }
    }
}

fn print_info_help() {
    println!("Usage: udevadm info [OPTIONS] <DEVICE>");
    println!();
    println!("Options:");
    println!("  -q, --query=TYPE    Query type: name, symlink, path, property, all");
    println!("  -p, --path=PATH     Sysfs device path");
    println!("  -n, --name=NAME     Device node name (/dev/...)");
    println!("  -a, --attribute-walk Walk sysfs attribute chain");
    println!("  -x, --export        Print in key=value format");
    println!("  -h, --help          Show this help");
}

/// Try to map a /dev/ name to a sysfs path by checking /sys/class/* and
/// /sys/block/*.
fn dev_name_to_syspath(name: &str) -> Option<String> {
    let bare = name.strip_prefix("/dev/").unwrap_or(name);

    // Check /sys/block/<name>.
    let block_path = format!("{SYS_DIR}/block/{bare}");
    if Path::new(&block_path).exists() {
        return Some(block_path);
    }

    // Check /sys/class/*/<name>.
    if let Ok(classes) = fs::read_dir(&format!("{SYS_DIR}/class")) {
        for cls in classes.flatten() {
            let candidate = cls.path().join(bare);
            if candidate.exists() {
                return Some(candidate.to_string_lossy().to_string());
            }
        }
    }

    // Fall back: treat as a sysfs path directly.
    Some(format!("{SYS_DIR}/devices/{bare}"))
}

/// `udevadm trigger` -- re-trigger device events for coldplug.
fn cmd_trigger(args: &[String]) -> i32 {
    let mut subsystem_match: Option<String> = None;
    let mut action = "change";
    let mut device_type: Option<String> = None;
    let mut verbose = false;
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "--subsystem-match" | "-s" => {
                i += 1;
                if i < args.len() {
                    subsystem_match = Some(args[i].clone());
                }
            }
            "--action" | "-c" => {
                i += 1;
                if i < args.len() {
                    action = leak_str(&args[i]);
                }
            }
            "--type" | "-t" => {
                i += 1;
                if i < args.len() {
                    device_type = Some(args[i].clone());
                }
            }
            "--verbose" | "-v" => verbose = true,
            "--help" | "-h" => {
                println!("Usage: udevadm trigger [OPTIONS]");
                println!();
                println!("Options:");
                println!("  -s, --subsystem-match=SUB  Trigger only for subsystem SUB");
                println!("  -c, --action=ACTION        Set action (default: change)");
                println!("  -t, --type=TYPE            Device type filter (devices/subsystems)");
                println!("  -v, --verbose              Print triggered devices");
                println!("  -h, --help                 Show this help");
                return 0;
            }
            s => {
                if let Some(sub) = s.strip_prefix("--subsystem-match=") {
                    subsystem_match = Some(sub.to_string());
                } else if let Some(a) = s.strip_prefix("--action=") {
                    action = leak_str(a);
                } else if let Some(t) = s.strip_prefix("--type=") {
                    device_type = Some(t.to_string());
                }
            }
        }
        i += 1;
    }

    let _ = device_type; // reserved for future filtering

    // Enumerate devices to trigger.
    let devices = if let Some(ref sub) = subsystem_match {
        enumerate_subsystem(sub)
    } else {
        enumerate_all_devices()
    };

    let mut count = 0u64;
    for devpath in &devices {
        // Write the action to the device's uevent file to trigger re-processing.
        let uevent_path = format!("{devpath}/uevent");
        if Path::new(&uevent_path).exists() {
            let _ = fs::write(&uevent_path, action);
            count += 1;
            if verbose {
                println!("{devpath}");
            }
        }
    }

    if verbose {
        eprintln!("Triggered {count} devices");
    }

    0
}

/// `udevadm settle` -- wait for pending udev events to complete.
fn cmd_settle(args: &[String]) -> i32 {
    let mut timeout = DEFAULT_SETTLE_TIMEOUT;
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "--timeout" | "-t" => {
                i += 1;
                if i < args.len() {
                    timeout = args[i].parse().unwrap_or(DEFAULT_SETTLE_TIMEOUT);
                }
            }
            "--help" | "-h" => {
                println!("Usage: udevadm settle [OPTIONS]");
                println!();
                println!("Options:");
                println!("  -t, --timeout=SEC  Maximum wait time (default: {DEFAULT_SETTLE_TIMEOUT})");
                println!("  -h, --help         Show this help");
                return 0;
            }
            s => {
                if let Some(t) = s.strip_prefix("--timeout=") {
                    timeout = t.parse().unwrap_or(DEFAULT_SETTLE_TIMEOUT);
                }
            }
        }
        i += 1;
    }

    // Check for pending events by looking at the udev queue.
    let queue_file = "/run/udev/queue";
    let mut waited = 0u64;

    loop {
        // If the queue file doesn't exist or is empty, all events are processed.
        match fs::read_to_string(queue_file) {
            Ok(content) if !content.trim().is_empty() => {
                // Still pending events.
                if waited >= timeout {
                    eprintln!("udevadm settle: timeout waiting for events");
                    return 1;
                }
                // Sleep 100ms (simulated -- in real code we'd use nanosleep).
                waited += 1;
            }
            _ => {
                // Queue empty or doesn't exist -- settled.
                return 0;
            }
        }
    }
}

/// `udevadm monitor` -- monitor device events.
fn cmd_monitor(args: &[String]) -> i32 {
    let mut show_properties = false;
    let mut filter_subsystem: Option<String> = None;
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "--property" | "-p" => show_properties = true,
            "--subsystem-match" | "-s" => {
                i += 1;
                if i < args.len() {
                    filter_subsystem = Some(args[i].clone());
                }
            }
            "--kernel" | "-k" => {
                // Kernel events only (default).
            }
            "--udev" | "-u" => {
                // Udev events only.
            }
            "--help" | "-h" => {
                println!("Usage: udevadm monitor [OPTIONS]");
                println!();
                println!("Options:");
                println!("  -p, --property             Show event properties");
                println!("  -s, --subsystem-match=SUB  Filter by subsystem");
                println!("  -k, --kernel               Show kernel events");
                println!("  -u, --udev                 Show udev events");
                println!("  -h, --help                 Show this help");
                return 0;
            }
            s => {
                if let Some(sub) = s.strip_prefix("--subsystem-match=") {
                    filter_subsystem = Some(sub.to_string());
                }
            }
        }
        i += 1;
    }

    println!("monitor will print device events as they occur.");
    println!("Listening for kernel uevents and udev events...");

    // In a real implementation, we would open a netlink socket and poll.
    // Here we simulate by scanning for new uevent files.
    let uevent_source = "/run/udev/uevent_source";
    loop {
        match fs::read_to_string(uevent_source) {
            Ok(content) if !content.trim().is_empty() => {
                // Parse events from the source file.
                for block in content.split("\n\n") {
                    if block.trim().is_empty() {
                        continue;
                    }
                    if let Some(event) = DeviceEvent::parse(block) {
                        // Apply subsystem filter.
                        if let Some(ref sub) = filter_subsystem {
                            if !glob_match(sub, &event.subsystem) {
                                continue;
                            }
                        }
                        println!("{}", event.format_monitor(show_properties));
                    }
                }
                // Clear the source file after processing.
                let _ = fs::write(uevent_source, "");
            }
            _ => {
                // No events -- would sleep here in a real daemon.
                break;
            }
        }
    }

    0
}

/// `udevadm control` -- send control messages to the running daemon.
fn cmd_control(args: &[String]) -> i32 {
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "--reload-rules" | "-R" | "--reload" => {
                // Write reload command to control socket.
                let _ = fs::create_dir_all("/run/udev");
                match fs::write(CONTROL_SOCKET, "reload\n") {
                    Ok(()) => {
                        println!("Requested rule reload");
                        return 0;
                    }
                    Err(e) => {
                        eprintln!("udevadm control: cannot write to {CONTROL_SOCKET}: {e}");
                        return 1;
                    }
                }
            }
            "--log-level" | "-l" => {
                i += 1;
                if i < args.len() {
                    return set_log_level(&args[i]);
                }
                eprintln!("udevadm control: --log-level requires an argument");
                return 1;
            }
            "--stop-exec-queue" => {
                let _ = fs::create_dir_all("/run/udev");
                let _ = fs::write(CONTROL_SOCKET, "stop-exec-queue\n");
                println!("Stopped exec queue");
                return 0;
            }
            "--start-exec-queue" => {
                let _ = fs::create_dir_all("/run/udev");
                let _ = fs::write(CONTROL_SOCKET, "start-exec-queue\n");
                println!("Started exec queue");
                return 0;
            }
            "--exit" => {
                let _ = fs::create_dir_all("/run/udev");
                let _ = fs::write(CONTROL_SOCKET, "exit\n");
                println!("Requested daemon exit");
                return 0;
            }
            "--help" | "-h" => {
                println!("Usage: udevadm control [OPTIONS]");
                println!();
                println!("Options:");
                println!("  -R, --reload-rules     Reload udev rules");
                println!("  -l, --log-level=LEVEL  Set log level (error/warning/info/debug)");
                println!("  --stop-exec-queue      Stop event execution");
                println!("  --start-exec-queue     Resume event execution");
                println!("  --exit                 Request daemon exit");
                println!("  -h, --help             Show this help");
                return 0;
            }
            s => {
                if let Some(level) = s.strip_prefix("--log-level=") {
                    return set_log_level(level);
                }
            }
        }
        i += 1;
    }

    eprintln!("udevadm control: no command specified (try --help)");
    1
}

fn set_log_level(level: &str) -> i32 {
    if LogLevel::from_str(level).is_none() {
        eprintln!("udevadm control: invalid log level '{level}'");
        return 1;
    }
    let _ = fs::create_dir_all("/run/udev");
    match fs::write(CONTROL_SOCKET, format!("log-level={level}\n")) {
        Ok(()) => {
            println!("Set log level to {level}");
            0
        }
        Err(e) => {
            eprintln!("udevadm control: {e}");
            1
        }
    }
}

// ============================================================================
// Daemon mode
// ============================================================================

/// Run the udevd daemon: load rules, listen for events, process them.
fn run_daemon(debug: bool, resolve_early: bool) -> i32 {
    let log_level = if debug { LogLevel::Debug } else { LogLevel::Info };

    if log_level >= LogLevel::Info {
        eprintln!("udevd v{VERSION}: starting device manager");
    }

    // Ensure run directories exist.
    let _ = fs::create_dir_all(RUN_DB_DIR);
    let _ = fs::create_dir_all(DEV_DIR);

    let mut state = DaemonState::new(log_level, resolve_early);

    if state.log_level >= LogLevel::Info {
        eprintln!("udevd: loaded {} rules from {RULES_DIR}", state.rules.len());
    }

    // Coldplug: enumerate existing devices and process them.
    coldplug(&mut state);

    // Single-pass event handling. In a real daemon this would be a poll() loop
    // over the netlink and control sockets, but the stub processes whatever's
    // pending in the control file + uevent source and returns.
    // Check for control commands.
    if let Ok(cmd) = fs::read_to_string(CONTROL_SOCKET) {
        let _ = fs::write(CONTROL_SOCKET, "");
        for line in cmd.lines() {
            let trimmed = line.trim();
            if trimmed == "reload" {
                state.reload_rules();
            } else if trimmed == "exit" {
                if state.log_level >= LogLevel::Info {
                    eprintln!("udevd: shutting down (processed {} events)", state.event_count);
                }
                return 0;
            } else if let Some(level) = trimmed.strip_prefix("log-level=") {
                if let Some(ll) = LogLevel::from_str(level) {
                    state.log_level = ll;
                    eprintln!("udevd: log level set to {}", ll.as_str());
                }
            } else if trimmed == "stop-exec-queue" {
                if state.log_level >= LogLevel::Info {
                    eprintln!("udevd: exec queue stopped");
                }
            } else if trimmed == "start-exec-queue" {
                if state.log_level >= LogLevel::Info {
                    eprintln!("udevd: exec queue started");
                }
            }
        }
    }

    // Read uevent source for new events.
    let uevent_source = "/run/udev/uevent_source";
    if let Ok(content) = fs::read_to_string(uevent_source) {
        if !content.trim().is_empty() {
            let _ = fs::write(uevent_source, "");
            for block in content.split("\n\n") {
                if block.trim().is_empty() {
                    continue;
                }
                if let Some(event) = DeviceEvent::parse(block) {
                    state.process_event(&event);
                }
            }
        }
    }

    if state.log_level >= LogLevel::Info {
        eprintln!(
            "udevd: event loop complete ({} events processed)",
            state.event_count,
        );
    }

    0
}

/// Coldplug: enumerate all existing devices and process add events for them.
fn coldplug(state: &mut DaemonState) {
    if state.log_level >= LogLevel::Info {
        eprintln!("udevd: starting coldplug enumeration");
    }

    let devices = enumerate_all_devices();
    let mut count = 0u64;

    for devpath in &devices {
        if let Some(mut event) = read_uevent_from_sysfs(devpath) {
            event.action = DeviceAction::Add;
            event.seqnum = count;
            state.process_event(&event);
            count += 1;
        }
    }

    if state.log_level >= LogLevel::Info {
        eprintln!("udevd: coldplug complete ({count} devices)");
    }
}

// ============================================================================
// Utility helpers
// ============================================================================

/// Leak a string into a `&'static str` for storing parsed CLI args.
/// Only used for small, bounded argument values.
fn leak_str(s: &str) -> &'static str {
    Box::leak(s.to_string().into_boxed_str())
}

// ============================================================================
// Entry point
// ============================================================================

fn run() -> i32 {
    let args: Vec<String> = env::args().collect();
    if args.is_empty() {
        eprintln!("udevd: no argv[0]");
        return 1;
    }

    // Determine personality from argv[0].
    let prog = args[0]
        .rsplit('/')
        .next()
        .unwrap_or(&args[0]);

    if prog == "udevadm" || (args.len() > 1 && args[1] == "--udevadm") {
        // udevadm mode: dispatch subcommand.
        let sub_start = if prog == "udevadm" { 1 } else { 2 };

        if args.len() <= sub_start {
            print_udevadm_help();
            return 0;
        }

        let subcmd = &args[sub_start];
        let sub_args = args[sub_start + 1..].to_vec();

        match subcmd.as_str() {
            "info" => cmd_info(&sub_args),
            "trigger" => cmd_trigger(&sub_args),
            "settle" => cmd_settle(&sub_args),
            "monitor" => cmd_monitor(&sub_args),
            "control" => cmd_control(&sub_args),
            "version" | "--version" | "-V" => {
                println!("udevadm {VERSION}");
                0
            }
            "--help" | "-h" | "help" => {
                print_udevadm_help();
                0
            }
            _ => {
                eprintln!("udevadm: unknown command '{subcmd}'");
                print_udevadm_help();
                1
            }
        }
    } else {
        // udevd daemon mode.
        let mut debug = false;
        let mut resolve_early = false;

        for arg in &args[1..] {
            match arg.as_str() {
                "--debug" | "-d" => debug = true,
                "--resolve-names=early" => resolve_early = true,
                "--resolve-names=late" => resolve_early = false,
                "--version" | "-V" => {
                    println!("udevd {VERSION}");
                    return 0;
                }
                "--help" | "-h" => {
                    print_daemon_help();
                    return 0;
                }
                _ => {
                    if arg.starts_with("--resolve-names=") {
                        // Accept but ignore unknown values.
                    } else {
                        eprintln!("udevd: unknown option '{arg}'");
                        return 1;
                    }
                }
            }
        }

        run_daemon(debug, resolve_early)
    }
}

fn print_udevadm_help() {
    println!("Usage: udevadm <COMMAND> [OPTIONS]");
    println!();
    println!("Commands:");
    println!("  info       Query device properties from sysfs and database");
    println!("  trigger    Request device events from the kernel");
    println!("  settle     Wait for pending udev events");
    println!("  monitor    Monitor device events in real time");
    println!("  control    Send control commands to udevd daemon");
    println!("  version    Print version");
    println!("  help       Show this help");
    println!();
    println!("Run 'udevadm <COMMAND> --help' for command-specific help.");
}

fn print_daemon_help() {
    println!("Usage: udevd [OPTIONS]");
    println!();
    println!("Options:");
    println!("  -d, --debug                 Run in foreground with debug output");
    println!("  --resolve-names=early|late  When to resolve user/group names");
    println!("  -V, --version               Print version");
    println!("  -h, --help                  Show this help");
}

#[cfg(not(test))]
#[unsafe(no_mangle)]
pub extern "C" fn main(_argc: i32, _argv: *const *const u8) -> i32 {
    run()
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ---- Glob matcher tests ----

    #[test]
    fn glob_exact_match() {
        assert!(glob_match("hello", "hello"));
    }

    #[test]
    fn glob_exact_no_match() {
        assert!(!glob_match("hello", "world"));
    }

    #[test]
    fn glob_star_match_all() {
        assert!(glob_match("*", "anything"));
    }

    #[test]
    fn glob_star_prefix() {
        assert!(glob_match("sd*", "sda"));
        assert!(glob_match("sd*", "sda1"));
        assert!(glob_match("sd*", "sdb"));
        assert!(!glob_match("sd*", "nvme0"));
    }

    #[test]
    fn glob_star_suffix() {
        assert!(glob_match("*.rules", "50-udev.rules"));
        assert!(!glob_match("*.rules", "50-udev.conf"));
    }

    #[test]
    fn glob_star_middle() {
        assert!(glob_match("sd*1", "sda1"));
        assert!(glob_match("sd*1", "sdb1"));
        assert!(!glob_match("sd*1", "sda2"));
    }

    #[test]
    fn glob_question_mark() {
        assert!(glob_match("sd?", "sda"));
        assert!(glob_match("sd?", "sdb"));
        assert!(!glob_match("sd?", "sda1"));
    }

    #[test]
    fn glob_char_class() {
        assert!(glob_match("sd[abc]", "sda"));
        assert!(glob_match("sd[abc]", "sdb"));
        assert!(glob_match("sd[abc]", "sdc"));
        assert!(!glob_match("sd[abc]", "sdd"));
    }

    #[test]
    fn glob_char_class_range() {
        assert!(glob_match("sd[a-d]", "sda"));
        assert!(glob_match("sd[a-d]", "sdd"));
        assert!(!glob_match("sd[a-d]", "sde"));
    }

    #[test]
    fn glob_char_class_negate() {
        assert!(!glob_match("sd[!a-c]", "sda"));
        assert!(glob_match("sd[!a-c]", "sdd"));
    }

    #[test]
    fn glob_empty_pattern_and_text() {
        assert!(glob_match("", ""));
    }

    #[test]
    fn glob_empty_pattern_nonempty_text() {
        assert!(!glob_match("", "a"));
    }

    #[test]
    fn glob_star_empty_text() {
        assert!(glob_match("*", ""));
    }

    #[test]
    fn glob_multiple_stars() {
        assert!(glob_match("*/*", "a/b"));
        assert!(glob_match("*-*-*", "by-id-serial"));
    }

    // ---- DeviceAction tests ----

    #[test]
    fn device_action_roundtrip() {
        for action_str in &["add", "remove", "change", "move", "online", "offline", "bind", "unbind"] {
            let action = DeviceAction::from_str(action_str).unwrap();
            assert_eq!(action.as_str(), *action_str);
        }
    }

    #[test]
    fn device_action_unknown() {
        assert!(DeviceAction::from_str("invalid").is_none());
    }

    // ---- LogLevel tests ----

    #[test]
    fn log_level_from_str() {
        assert_eq!(LogLevel::from_str("error"), Some(LogLevel::Error));
        assert_eq!(LogLevel::from_str("err"), Some(LogLevel::Error));
        assert_eq!(LogLevel::from_str("warning"), Some(LogLevel::Warning));
        assert_eq!(LogLevel::from_str("warn"), Some(LogLevel::Warning));
        assert_eq!(LogLevel::from_str("info"), Some(LogLevel::Info));
        assert_eq!(LogLevel::from_str("debug"), Some(LogLevel::Debug));
        assert_eq!(LogLevel::from_str("0"), Some(LogLevel::Error));
        assert_eq!(LogLevel::from_str("3"), Some(LogLevel::Debug));
        assert!(LogLevel::from_str("invalid").is_none());
    }

    #[test]
    fn log_level_ordering() {
        assert!(LogLevel::Error < LogLevel::Warning);
        assert!(LogLevel::Warning < LogLevel::Info);
        assert!(LogLevel::Info < LogLevel::Debug);
    }

    #[test]
    fn log_level_as_str_roundtrip() {
        for (s, expected) in [("error", LogLevel::Error), ("debug", LogLevel::Debug)] {
            let level = LogLevel::from_str(s).unwrap();
            assert_eq!(level, expected);
            assert_eq!(level.as_str(), s);
        }
    }

    // ---- DeviceEvent tests ----

    #[test]
    fn parse_uevent_basic() {
        let text = "ACTION=add\nDEVPATH=/devices/pci/sda\nSUBSYSTEM=block\nMAJOR=8\nMINOR=0\nSEQNUM=42\n";
        let ev = DeviceEvent::parse(text).unwrap();
        assert_eq!(ev.action, DeviceAction::Add);
        assert_eq!(ev.devpath, "/devices/pci/sda");
        assert_eq!(ev.subsystem, "block");
        assert_eq!(ev.major, 8);
        assert_eq!(ev.minor, 0);
        assert_eq!(ev.seqnum, 42);
        assert_eq!(ev.kernel_name, "sda");
    }

    #[test]
    fn parse_uevent_with_devname() {
        let text = "ACTION=add\nDEVPATH=/devices/eth0\nSUBSYSTEM=net\nDEVNAME=eth0\nSEQNUM=1\n";
        let ev = DeviceEvent::parse(text).unwrap();
        assert_eq!(ev.kernel_name, "eth0");
    }

    #[test]
    fn parse_uevent_extra_env() {
        let text = "ACTION=add\nDEVPATH=/devices/sda\nSUBSYSTEM=block\nID_MODEL=Virtual_Disk\n";
        let ev = DeviceEvent::parse(text).unwrap();
        assert_eq!(ev.env.get("ID_MODEL").map(|s| s.as_str()), Some("Virtual_Disk"));
    }

    #[test]
    fn parse_uevent_invalid_action() {
        let text = "ACTION=frobnicate\nDEVPATH=/x\n";
        assert!(DeviceEvent::parse(text).is_none());
    }

    #[test]
    fn event_format_monitor_basic() {
        let ev = DeviceEvent {
            action: DeviceAction::Add,
            devpath: "/devices/sda".to_string(),
            subsystem: "block".to_string(),
            kernel_name: "sda".to_string(),
            devtype: String::new(),
            major: 8,
            minor: 0,
            seqnum: 100,
            env: HashMap::new(),
        };
        let out = ev.format_monitor(false);
        assert!(out.contains("add"));
        assert!(out.contains("/devices/sda"));
        assert!(out.contains("block"));
    }

    #[test]
    fn event_format_monitor_with_properties() {
        let ev = DeviceEvent {
            action: DeviceAction::Remove,
            devpath: "/devices/sdb".to_string(),
            subsystem: "block".to_string(),
            kernel_name: "sdb".to_string(),
            devtype: "disk".to_string(),
            major: 8,
            minor: 16,
            seqnum: 200,
            env: HashMap::new(),
        };
        let out = ev.format_monitor(true);
        assert!(out.contains("ACTION=remove"));
        assert!(out.contains("DEVPATH=/devices/sdb"));
        assert!(out.contains("MAJOR=8"));
        assert!(out.contains("MINOR=16"));
        assert!(out.contains("DEVTYPE=disk"));
    }

    // ---- Rule tokenizer tests ----

    #[test]
    fn tokenize_simple_rule() {
        let tokens = tokenize_rule(r#"KERNEL=="sda", NAME="mydisk""#);
        assert_eq!(tokens.len(), 2);
        assert!(tokens[0].contains("KERNEL"));
        assert!(tokens[1].contains("NAME"));
    }

    #[test]
    fn tokenize_quoted_comma() {
        let tokens = tokenize_rule(r#"KERNEL=="a,b", NAME="x""#);
        assert_eq!(tokens.len(), 2);
        assert!(tokens[0].contains("a,b"));
    }

    #[test]
    fn tokenize_empty() {
        let tokens = tokenize_rule("");
        assert!(tokens.is_empty());
    }

    // ---- Rule key-op-value parsing ----

    #[test]
    fn parse_kov_match() {
        let result = parse_key_op_value(r#"KERNEL=="sda""#);
        assert!(result.is_some());
        let (k, op, v) = result.unwrap();
        assert_eq!(k, "KERNEL");
        assert_eq!(op, "==");
        assert_eq!(v, "sda");
    }

    #[test]
    fn parse_kov_not_match() {
        let result = parse_key_op_value(r#"SUBSYSTEM!="net""#);
        assert!(result.is_some());
        let (k, op, v) = result.unwrap();
        assert_eq!(k, "SUBSYSTEM");
        assert_eq!(op, "!=");
        assert_eq!(v, "net");
    }

    #[test]
    fn parse_kov_assign() {
        let result = parse_key_op_value(r#"NAME="mydev""#);
        assert!(result.is_some());
        let (k, op, v) = result.unwrap();
        assert_eq!(k, "NAME");
        assert_eq!(op, "=");
        assert_eq!(v, "mydev");
    }

    #[test]
    fn parse_kov_append() {
        let result = parse_key_op_value(r#"SYMLINK+="disk/mylink""#);
        assert!(result.is_some());
        let (k, op, v) = result.unwrap();
        assert_eq!(k, "SYMLINK");
        assert_eq!(op, "+=");
        assert_eq!(v, "disk/mylink");
    }

    #[test]
    fn parse_kov_attr_brace() {
        let result = parse_key_op_value(r#"ATTR{size}=="1024""#);
        assert!(result.is_some());
        let (k, op, v) = result.unwrap();
        assert_eq!(k, "ATTR{size}");
        assert_eq!(op, "==");
        assert_eq!(v, "1024");
    }

    // ---- extract_brace_arg tests ----

    #[test]
    fn brace_arg_basic() {
        assert_eq!(extract_brace_arg("ATTR{size}", "ATTR"), Some("size".to_string()));
    }

    #[test]
    fn brace_arg_env() {
        assert_eq!(extract_brace_arg("ENV{ID_MODEL}", "ENV"), Some("ID_MODEL".to_string()));
    }

    #[test]
    fn brace_arg_no_match() {
        assert_eq!(extract_brace_arg("KERNEL", "ATTR"), None);
    }

    #[test]
    fn brace_arg_no_brace() {
        assert_eq!(extract_brace_arg("ATTR", "ATTR"), None);
    }

    #[test]
    fn brace_arg_unclosed() {
        assert_eq!(extract_brace_arg("ATTR{size", "ATTR"), None);
    }

    // ---- Rule parsing tests ----

    #[test]
    fn parse_simple_rule() {
        let rule = parse_rule_line(
            r#"KERNEL=="sda", NAME="mydisk""#,
            "test.rules",
            1,
        );
        assert!(rule.is_some());
        let r = rule.unwrap();
        assert_eq!(r.matches.len(), 1);
        assert_eq!(r.assigns.len(), 1);
    }

    #[test]
    fn parse_rule_with_multiple_conditions() {
        let rule = parse_rule_line(
            r#"KERNEL=="sd*", SUBSYSTEM=="block", ACTION=="add", NAME="%k", MODE="0660""#,
            "test.rules",
            5,
        );
        assert!(rule.is_some());
        let r = rule.unwrap();
        assert_eq!(r.matches.len(), 3);
        assert_eq!(r.assigns.len(), 2);
    }

    #[test]
    fn parse_rule_with_attr() {
        let rule = parse_rule_line(
            r#"KERNEL=="sd*", ATTR{removable}=="1", NAME="removable/%k""#,
            "test.rules",
            10,
        );
        assert!(rule.is_some());
        let r = rule.unwrap();
        assert_eq!(r.matches.len(), 2);
    }

    #[test]
    fn parse_rule_with_symlink_and_run() {
        let rule = parse_rule_line(
            r#"KERNEL=="sda", SYMLINK+="mydisk", RUN+="/sbin/on-disk-add""#,
            "test.rules",
            15,
        );
        assert!(rule.is_some());
        let r = rule.unwrap();
        assert_eq!(r.assigns.len(), 2);
    }

    #[test]
    fn parse_comment_line() {
        let mut rules = Vec::new();
        parse_rules_file("# this is a comment\n", "test.rules", &mut rules);
        assert!(rules.is_empty());
    }

    #[test]
    fn parse_empty_line() {
        let mut rules = Vec::new();
        parse_rules_file("\n\n\n", "test.rules", &mut rules);
        assert!(rules.is_empty());
    }

    #[test]
    fn parse_continuation_line() {
        let mut rules = Vec::new();
        parse_rules_file(
            "KERNEL==\"sda\", \\\n  NAME=\"mydisk\"\n",
            "test.rules",
            &mut rules,
        );
        assert_eq!(rules.len(), 1);
    }

    #[test]
    fn parse_rule_goto_label() {
        let rule = parse_rule_line(
            r#"KERNEL=="loop*", GOTO="skip_loop""#,
            "test.rules",
            1,
        );
        assert!(rule.is_some());
        let r = rule.unwrap();
        assert!(r.assigns.iter().any(|a| matches!(a, AssignKey::Goto(t) if t == "skip_loop")));
    }

    // ---- Rule evaluation tests ----

    #[test]
    fn rule_matches_kernel_name() {
        let rules = vec![Rule {
            matches: vec![MatchKey::Kernel("sda".to_string())],
            assigns: vec![AssignKey::Name("my_sda".to_string())],
            _source_file: "test".to_string(),
            _source_line: 1,
        }];

        let mut ev = DeviceEvent::new();
        ev.kernel_name = "sda".to_string();
        ev.action = DeviceAction::Add;

        let result = apply_rules(&rules, &ev);
        assert_eq!(result.name.as_deref(), Some("my_sda"));
    }

    #[test]
    fn rule_no_match_kernel_name() {
        let rules = vec![Rule {
            matches: vec![MatchKey::Kernel("sda".to_string())],
            assigns: vec![AssignKey::Name("my_sda".to_string())],
            _source_file: "test".to_string(),
            _source_line: 1,
        }];

        let mut ev = DeviceEvent::new();
        ev.kernel_name = "sdb".to_string();

        let result = apply_rules(&rules, &ev);
        assert!(result.name.is_none());
    }

    #[test]
    fn rule_glob_kernel() {
        let rules = vec![Rule {
            matches: vec![MatchKey::Kernel("sd*".to_string())],
            assigns: vec![AssignKey::Name("disk_%k".to_string())],
            _source_file: "test".to_string(),
            _source_line: 1,
        }];

        let mut ev = DeviceEvent::new();
        ev.kernel_name = "sda".to_string();

        let result = apply_rules(&rules, &ev);
        assert!(result.name.is_some());
    }

    #[test]
    fn rule_subsystem_match() {
        let rules = vec![Rule {
            matches: vec![
                MatchKey::Kernel("sd*".to_string()),
                MatchKey::Subsystem("block".to_string()),
            ],
            assigns: vec![AssignKey::Name("blk_%k".to_string())],
            _source_file: "test".to_string(),
            _source_line: 1,
        }];

        let mut ev = DeviceEvent::new();
        ev.kernel_name = "sda".to_string();
        ev.subsystem = "block".to_string();

        let result = apply_rules(&rules, &ev);
        assert!(result.name.is_some());
    }

    #[test]
    fn rule_subsystem_no_match() {
        let rules = vec![Rule {
            matches: vec![
                MatchKey::Kernel("sd*".to_string()),
                MatchKey::Subsystem("net".to_string()),
            ],
            assigns: vec![AssignKey::Name("net_%k".to_string())],
            _source_file: "test".to_string(),
            _source_line: 1,
        }];

        let mut ev = DeviceEvent::new();
        ev.kernel_name = "sda".to_string();
        ev.subsystem = "block".to_string();

        let result = apply_rules(&rules, &ev);
        assert!(result.name.is_none());
    }

    #[test]
    fn rule_action_match() {
        let rules = vec![Rule {
            matches: vec![MatchKey::Action("add".to_string())],
            assigns: vec![AssignKey::Name("added".to_string())],
            _source_file: "test".to_string(),
            _source_line: 1,
        }];

        let mut ev = DeviceEvent::new();
        ev.action = DeviceAction::Add;

        let result = apply_rules(&rules, &ev);
        assert_eq!(result.name.as_deref(), Some("added"));
    }

    #[test]
    fn rule_negated_match() {
        let rules = vec![Rule {
            matches: vec![MatchKey::SubsystemNot("net".to_string())],
            assigns: vec![AssignKey::Name("not_net".to_string())],
            _source_file: "test".to_string(),
            _source_line: 1,
        }];

        let mut ev = DeviceEvent::new();
        ev.subsystem = "block".to_string();

        let result = apply_rules(&rules, &ev);
        assert_eq!(result.name.as_deref(), Some("not_net"));
    }

    #[test]
    fn rule_env_match() {
        let rules = vec![Rule {
            matches: vec![MatchKey::Env("ID_TYPE".to_string(), "disk".to_string())],
            assigns: vec![AssignKey::Name("disk_dev".to_string())],
            _source_file: "test".to_string(),
            _source_line: 1,
        }];

        let mut ev = DeviceEvent::new();
        ev.env.insert("ID_TYPE".to_string(), "disk".to_string());

        let result = apply_rules(&rules, &ev);
        assert_eq!(result.name.as_deref(), Some("disk_dev"));
    }

    #[test]
    fn rule_multiple_rules_last_wins() {
        let rules = vec![
            Rule {
                matches: vec![MatchKey::Kernel("sd*".to_string())],
                assigns: vec![AssignKey::Name("first".to_string())],
                _source_file: "test".to_string(),
                _source_line: 1,
            },
            Rule {
                matches: vec![MatchKey::Kernel("sda".to_string())],
                assigns: vec![AssignKey::Name("second".to_string())],
                _source_file: "test".to_string(),
                _source_line: 2,
            },
        ];

        let mut ev = DeviceEvent::new();
        ev.kernel_name = "sda".to_string();

        let result = apply_rules(&rules, &ev);
        assert_eq!(result.name.as_deref(), Some("second"));
    }

    #[test]
    fn rule_symlinks_accumulate() {
        let rules = vec![
            Rule {
                matches: vec![MatchKey::Kernel("sda".to_string())],
                assigns: vec![AssignKey::Symlink("link1".to_string())],
                _source_file: "test".to_string(),
                _source_line: 1,
            },
            Rule {
                matches: vec![MatchKey::Kernel("sda".to_string())],
                assigns: vec![AssignKey::Symlink("link2".to_string())],
                _source_file: "test".to_string(),
                _source_line: 2,
            },
        ];

        let mut ev = DeviceEvent::new();
        ev.kernel_name = "sda".to_string();

        let result = apply_rules(&rules, &ev);
        assert_eq!(result.symlinks, vec!["link1", "link2"]);
    }

    #[test]
    fn rule_goto_skips_rules() {
        let rules = vec![
            Rule {
                matches: vec![MatchKey::Kernel("loop*".to_string())],
                assigns: vec![AssignKey::Goto("end".to_string())],
                _source_file: "test".to_string(),
                _source_line: 1,
            },
            Rule {
                matches: vec![MatchKey::Kernel("loop*".to_string())],
                assigns: vec![AssignKey::Name("should_not_set".to_string())],
                _source_file: "test".to_string(),
                _source_line: 2,
            },
            Rule {
                matches: vec![],
                assigns: vec![AssignKey::Label("end".to_string())],
                _source_file: "test".to_string(),
                _source_line: 3,
            },
        ];

        let mut ev = DeviceEvent::new();
        ev.kernel_name = "loop0".to_string();

        let result = apply_rules(&rules, &ev);
        assert!(result.name.is_none());
    }

    // ---- Substitution tests ----

    #[test]
    fn substitute_kernel_name() {
        let ev = DeviceEvent {
            action: DeviceAction::Add,
            devpath: "/devices/sda".to_string(),
            subsystem: "block".to_string(),
            kernel_name: "sda".to_string(),
            devtype: String::new(),
            major: 8,
            minor: 0,
            seqnum: 0,
            env: HashMap::new(),
        };
        let result = RuleResult::new();
        assert_eq!(substitute_value("%k", &ev, &result), "sda");
    }

    #[test]
    fn substitute_major_minor() {
        let ev = DeviceEvent {
            action: DeviceAction::Add,
            devpath: "".to_string(),
            subsystem: "".to_string(),
            kernel_name: "".to_string(),
            devtype: String::new(),
            major: 8,
            minor: 16,
            seqnum: 0,
            env: HashMap::new(),
        };
        let result = RuleResult::new();
        assert_eq!(substitute_value("%M:%m", &ev, &result), "8:16");
    }

    #[test]
    fn substitute_number_suffix() {
        let ev = DeviceEvent {
            action: DeviceAction::Add,
            devpath: "".to_string(),
            subsystem: "".to_string(),
            kernel_name: "sda1".to_string(),
            devtype: String::new(),
            major: 0,
            minor: 0,
            seqnum: 0,
            env: HashMap::new(),
        };
        let result = RuleResult::new();
        assert_eq!(substitute_value("part%n", &ev, &result), "part1");
    }

    #[test]
    fn substitute_env_var() {
        let mut env = HashMap::new();
        env.insert("ID_SERIAL".to_string(), "VBOX_HARDDISK".to_string());
        let ev = DeviceEvent {
            action: DeviceAction::Add,
            devpath: "".to_string(),
            subsystem: "".to_string(),
            kernel_name: "sda".to_string(),
            devtype: String::new(),
            major: 0,
            minor: 0,
            seqnum: 0,
            env,
        };
        let result = RuleResult::new();
        assert_eq!(
            substitute_value("by-id/%E{ID_SERIAL}", &ev, &result),
            "by-id/VBOX_HARDDISK"
        );
    }

    #[test]
    fn substitute_dollar_kernel() {
        let ev = DeviceEvent {
            action: DeviceAction::Add,
            devpath: "".to_string(),
            subsystem: "".to_string(),
            kernel_name: "sda".to_string(),
            devtype: String::new(),
            major: 0,
            minor: 0,
            seqnum: 0,
            env: HashMap::new(),
        };
        let result = RuleResult::new();
        assert_eq!(substitute_value("dev_$kernel", &ev, &result), "dev_sda");
    }

    #[test]
    fn substitute_double_percent() {
        let ev = DeviceEvent::new();
        let result = RuleResult::new();
        assert_eq!(substitute_value("100%%", &ev, &result), "100%");
    }

    #[test]
    fn substitute_devpath() {
        let ev = DeviceEvent {
            action: DeviceAction::Add,
            devpath: "/devices/pci/sda".to_string(),
            subsystem: "".to_string(),
            kernel_name: "".to_string(),
            devtype: String::new(),
            major: 0,
            minor: 0,
            seqnum: 0,
            env: HashMap::new(),
        };
        let result = RuleResult::new();
        assert_eq!(substitute_value("%p", &ev, &result), "/devices/pci/sda");
    }

    // ---- Sanitize tests ----

    #[test]
    fn sanitize_simple() {
        assert_eq!(sanitize_for_devname("Virtual Disk"), "Virtual_Disk");
    }

    #[test]
    fn sanitize_special_chars() {
        assert_eq!(sanitize_for_devname("a@b#c$d"), "a_b_c_d");
    }

    #[test]
    fn sanitize_leading_trailing_underscores() {
        assert_eq!(sanitize_for_devname(" hello "), "hello");
    }

    #[test]
    fn sanitize_consecutive_specials() {
        assert_eq!(sanitize_for_devname("a   b"), "a_b");
    }

    #[test]
    fn sanitize_dashes_and_dots_preserved() {
        assert_eq!(sanitize_for_devname("a-b.c"), "a-b.c");
    }

    // ---- Persistent naming tests ----

    #[test]
    fn persistent_links_block_by_path() {
        let ev = DeviceEvent {
            action: DeviceAction::Add,
            devpath: "/devices/pci0000:00/sda".to_string(),
            subsystem: "block".to_string(),
            kernel_name: "sda".to_string(),
            devtype: String::new(),
            major: 8,
            minor: 0,
            seqnum: 0,
            env: HashMap::new(),
        };
        let links = compute_persistent_links(&ev);
        assert!(links.iter().any(|l| l.starts_with("disk/by-path/")));
    }

    #[test]
    fn persistent_links_by_uuid() {
        let mut env = HashMap::new();
        env.insert("ID_FS_UUID".to_string(), "abcd-1234".to_string());
        let ev = DeviceEvent {
            action: DeviceAction::Add,
            devpath: "/devices/sda1".to_string(),
            subsystem: "block".to_string(),
            kernel_name: "sda1".to_string(),
            devtype: String::new(),
            major: 8,
            minor: 1,
            seqnum: 0,
            env,
        };
        let links = compute_persistent_links(&ev);
        assert!(links.iter().any(|l| l == "disk/by-uuid/abcd-1234"));
    }

    #[test]
    fn persistent_links_by_label() {
        let mut env = HashMap::new();
        env.insert("ID_FS_LABEL".to_string(), "MyData".to_string());
        let ev = DeviceEvent {
            action: DeviceAction::Add,
            devpath: "/devices/sdb1".to_string(),
            subsystem: "block".to_string(),
            kernel_name: "sdb1".to_string(),
            devtype: String::new(),
            major: 8,
            minor: 17,
            seqnum: 0,
            env,
        };
        let links = compute_persistent_links(&ev);
        assert!(links.iter().any(|l| l == "disk/by-label/MyData"));
    }

    #[test]
    fn persistent_links_non_block_empty() {
        let ev = DeviceEvent {
            action: DeviceAction::Add,
            devpath: "/devices/eth0".to_string(),
            subsystem: "net".to_string(),
            kernel_name: "eth0".to_string(),
            devtype: String::new(),
            major: 0,
            minor: 0,
            seqnum: 0,
            env: HashMap::new(),
        };
        let links = compute_persistent_links(&ev);
        // Net devices don't get disk/by-path links.
        assert!(!links.iter().any(|l| l.starts_with("disk/by-path")));
    }

    #[test]
    fn persistent_links_by_partuuid() {
        let mut env = HashMap::new();
        env.insert(
            "ID_PART_ENTRY_UUID".to_string(),
            "12345678-abcd-ef01-2345-6789abcdef01".to_string(),
        );
        let ev = DeviceEvent {
            action: DeviceAction::Add,
            devpath: "/devices/sda2".to_string(),
            subsystem: "block".to_string(),
            kernel_name: "sda2".to_string(),
            devtype: String::new(),
            major: 8,
            minor: 2,
            seqnum: 0,
            env,
        };
        let links = compute_persistent_links(&ev);
        assert!(links.iter().any(|l| l.starts_with("disk/by-partuuid/")));
    }

    // ---- Database tests ----

    #[test]
    fn db_key_for_device() {
        assert_eq!(DeviceDatabase::dev_key(8, 0), "b8:0");
        assert_eq!(DeviceDatabase::dev_key(259, 1), "b259:1");
    }

    #[test]
    fn db_key_for_net() {
        assert_eq!(DeviceDatabase::net_key(2), "n2");
    }

    // ---- make_match_key / make_assign_key tests ----

    #[test]
    fn make_match_kernel() {
        let m = make_match_key("KERNEL", "sda", false);
        assert!(matches!(m, Some(MatchKey::Kernel(ref p)) if p == "sda"));
    }

    #[test]
    fn make_match_kernel_negate() {
        let m = make_match_key("KERNEL", "sda", true);
        assert!(matches!(m, Some(MatchKey::KernelNot(ref p)) if p == "sda"));
    }

    #[test]
    fn make_match_attr() {
        let m = make_match_key("ATTR{size}", "1024", false);
        assert!(matches!(m, Some(MatchKey::Attr(ref a, ref v)) if a == "size" && v == "1024"));
    }

    #[test]
    fn make_assign_name() {
        let a = make_assign_key("NAME", "mydev", "=");
        assert!(matches!(a, Some(AssignKey::Name(ref v)) if v == "mydev"));
    }

    #[test]
    fn make_assign_symlink() {
        let a = make_assign_key("SYMLINK", "mylink", "+=");
        assert!(matches!(a, Some(AssignKey::Symlink(ref v)) if v == "mylink"));
    }

    #[test]
    fn make_assign_mode() {
        let a = make_assign_key("MODE", "0660", "=");
        assert!(matches!(a, Some(AssignKey::Mode(ref v)) if v == "0660"));
    }

    #[test]
    fn make_assign_env() {
        let a = make_assign_key("ENV{MY_VAR}", "hello", "=");
        assert!(matches!(a, Some(AssignKey::EnvSet(ref k, ref v)) if k == "MY_VAR" && v == "hello"));
    }

    #[test]
    fn make_assign_unknown() {
        let a = make_assign_key("BOGUS", "val", "=");
        assert!(a.is_none());
    }

    // ---- DaemonState / process_event tests ----

    #[test]
    fn daemon_state_process_event() {
        let mut state = DaemonState {
            rules: vec![Rule {
                matches: vec![MatchKey::Kernel("test*".to_string())],
                assigns: vec![AssignKey::Name("test_dev".to_string())],
                _source_file: "test".to_string(),
                _source_line: 1,
            }],
            db: DeviceDatabase::new("/tmp/udevd_test_db_nonexistent"),
            log_level: LogLevel::Error,
            event_count: 0,
            _resolve_names_early: false,
        };

        let ev = DeviceEvent {
            action: DeviceAction::Add,
            devpath: "/devices/test0".to_string(),
            subsystem: "misc".to_string(),
            kernel_name: "test0".to_string(),
            devtype: String::new(),
            major: 10,
            minor: 200,
            seqnum: 1,
            env: HashMap::new(),
        };

        state.process_event(&ev);
        assert_eq!(state.event_count, 1);
    }

    #[test]
    fn daemon_reload_rules() {
        let mut state = DaemonState {
            rules: Vec::new(),
            db: DeviceDatabase::new("/tmp/udevd_test_db_nonexistent2"),
            log_level: LogLevel::Error,
            event_count: 0,
            _resolve_names_early: false,
        };
        // Reload from a nonexistent directory produces 0 rules -- no crash.
        state.reload_rules();
        assert!(state.rules.is_empty());
    }

    // ---- Full rule file parsing tests ----

    #[test]
    fn parse_full_rules_file() {
        let content = r#"
# Block devices
KERNEL=="sd[a-z]", SUBSYSTEM=="block", NAME="%k", MODE="0660", GROUP="disk"
KERNEL=="sd[a-z][0-9]*", SUBSYSTEM=="block", SYMLINK+="disk/by-kernel/%k"

# Network devices
KERNEL=="eth*", SUBSYSTEM=="net", NAME="net/%k"

# Skip loop devices
KERNEL=="loop*", GOTO="end_loop"
KERNEL=="loop*", NAME="should_not_appear"
LABEL="end_loop"
"#;
        let mut rules = Vec::new();
        parse_rules_file(content, "test.rules", &mut rules);
        assert_eq!(rules.len(), 6); // 2 block + 1 net + 1 goto + 1 name + 1 label
    }

    // ---- Integration: full event processing through rules ----

    #[test]
    fn integration_block_device_event() {
        let rules = vec![
            Rule {
                matches: vec![
                    MatchKey::Kernel("sd[a-z]".to_string()),
                    MatchKey::Subsystem("block".to_string()),
                    MatchKey::Action("add".to_string()),
                ],
                assigns: vec![
                    AssignKey::Name("%k".to_string()),
                    AssignKey::Mode("0660".to_string()),
                    AssignKey::Group("disk".to_string()),
                    AssignKey::Symlink("disk/by-kernel/%k".to_string()),
                ],
                _source_file: "50-block.rules".to_string(),
                _source_line: 1,
            },
        ];

        let ev = DeviceEvent {
            action: DeviceAction::Add,
            devpath: "/devices/pci0000:00/sda".to_string(),
            subsystem: "block".to_string(),
            kernel_name: "sda".to_string(),
            devtype: "disk".to_string(),
            major: 8,
            minor: 0,
            seqnum: 1,
            env: HashMap::new(),
        };

        let result = apply_rules(&rules, &ev);
        assert_eq!(result.name.as_deref(), Some("sda"));
        assert_eq!(result.mode.as_deref(), Some("0660"));
        assert_eq!(result.group.as_deref(), Some("disk"));
        assert_eq!(result.symlinks, vec!["disk/by-kernel/sda"]);
    }

    #[test]
    fn integration_net_device_event() {
        let rules = vec![Rule {
            matches: vec![
                MatchKey::Subsystem("net".to_string()),
                MatchKey::Action("add".to_string()),
            ],
            assigns: vec![
                AssignKey::Name("net/%k".to_string()),
                AssignKey::EnvSet("NM_MANAGED".to_string(), "1".to_string()),
            ],
            _source_file: "70-net.rules".to_string(),
            _source_line: 1,
        }];

        let ev = DeviceEvent {
            action: DeviceAction::Add,
            devpath: "/devices/pci0000:00/eth0".to_string(),
            subsystem: "net".to_string(),
            kernel_name: "eth0".to_string(),
            devtype: String::new(),
            major: 0,
            minor: 0,
            seqnum: 10,
            env: HashMap::new(),
        };

        let result = apply_rules(&rules, &ev);
        assert_eq!(result.name.as_deref(), Some("net/eth0"));
        assert_eq!(result.extra_env.get("NM_MANAGED").map(|s| s.as_str()), Some("1"));
    }

    #[test]
    fn integration_run_command() {
        let rules = vec![Rule {
            matches: vec![MatchKey::Kernel("usb*".to_string())],
            assigns: vec![AssignKey::Run("/sbin/usb-handler %k".to_string())],
            _source_file: "test".to_string(),
            _source_line: 1,
        }];

        let ev = DeviceEvent {
            action: DeviceAction::Add,
            devpath: "/devices/usb1".to_string(),
            subsystem: "usb".to_string(),
            kernel_name: "usb1".to_string(),
            devtype: String::new(),
            major: 189,
            minor: 0,
            seqnum: 20,
            env: HashMap::new(),
        };

        let result = apply_rules(&rules, &ev);
        assert_eq!(result.run_cmds, vec!["/sbin/usb-handler usb1"]);
    }

    #[test]
    fn integration_tag_assignment() {
        let rules = vec![Rule {
            matches: vec![MatchKey::Subsystem("block".to_string())],
            assigns: vec![AssignKey::Tag("systemd".to_string())],
            _source_file: "test".to_string(),
            _source_line: 1,
        }];

        let mut ev = DeviceEvent::new();
        ev.subsystem = "block".to_string();

        let result = apply_rules(&rules, &ev);
        assert_eq!(result.tags, vec!["systemd"]);
    }

    #[test]
    fn integration_options_assignment() {
        let rules = vec![Rule {
            matches: vec![MatchKey::Kernel("sr*".to_string())],
            assigns: vec![AssignKey::Options("link_priority=-100".to_string())],
            _source_file: "test".to_string(),
            _source_line: 1,
        }];

        let mut ev = DeviceEvent::new();
        ev.kernel_name = "sr0".to_string();

        let result = apply_rules(&rules, &ev);
        assert_eq!(result.options, vec!["link_priority=-100"]);
    }

    // ---- check_match tests ----

    #[test]
    fn check_match_devpath() {
        let ev = DeviceEvent {
            action: DeviceAction::Add,
            devpath: "/devices/pci0000:00/sda".to_string(),
            subsystem: "block".to_string(),
            kernel_name: "sda".to_string(),
            devtype: String::new(),
            major: 8,
            minor: 0,
            seqnum: 0,
            env: HashMap::new(),
        };

        assert!(check_match(&MatchKey::DevPath("/devices/*/sda".to_string()), &ev));
        assert!(!check_match(&MatchKey::DevPathNot("/devices/*/sda".to_string()), &ev));
    }

    #[test]
    fn check_match_driver() {
        let mut env = HashMap::new();
        env.insert("DRIVER".to_string(), "ahci".to_string());
        let ev = DeviceEvent {
            action: DeviceAction::Add,
            devpath: "".to_string(),
            subsystem: "".to_string(),
            kernel_name: "".to_string(),
            devtype: String::new(),
            major: 0,
            minor: 0,
            seqnum: 0,
            env,
        };

        assert!(check_match(&MatchKey::Driver("ahci".to_string()), &ev));
        assert!(!check_match(&MatchKey::Driver("nvme".to_string()), &ev));
        assert!(check_match(&MatchKey::DriverNot("nvme".to_string()), &ev));
    }

    // ---- cli personality tests ----

    #[test]
    fn udevadm_argv0_detection() {
        // Simulate what `run()` does: extract program name from path.
        let path = "/usr/bin/udevadm";
        let prog = path.rsplit('/').next().unwrap_or(path);
        assert_eq!(prog, "udevadm");
    }

    #[test]
    fn udevd_argv0_detection() {
        let path = "/usr/sbin/udevd";
        let prog = path.rsplit('/').next().unwrap_or(path);
        assert_eq!(prog, "udevd");
    }
}
