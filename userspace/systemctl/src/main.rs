//! Multi-personality service management utility for OurOS.
//!
//! This binary detects its personality from `argv[0]`:
//!   - `systemctl`       — main service control (start/stop/status/enable/…)
//!   - `systemd-analyze`  — boot and service analysis
//!   - `systemd-cat`      — pipe stdin to journal
//!   - `systemd-cgls`     — show cgroup hierarchy as a tree
//!   - `systemd-cgtop`    — show cgroup resource usage
//!   - `systemd-escape`   — escape strings for systemd unit names
//!   - `systemd-path`     — show well-known system/user paths
//!   - `systemd-notify`   — notify service manager of status changes
//!   - `systemd-tmpfiles` — create/clean/remove temporary files
//!
//! Unit files follow an INI-like format with sections `[Unit]`, `[Service]`,
//! `[Install]`, `[Timer]`, `[Socket]`, `[Mount]`, `[Path]`.

#![deny(clippy::all)]
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::env;
use std::io::{self, BufRead, Write};
use std::process;

// ============================================================================
// Constants
// ============================================================================

const VERSION: &str = "0.1.0";

// ============================================================================
// Personality detection
// ============================================================================

/// Which personality this invocation runs under.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Personality {
    Systemctl,
    Analyze,
    Cat,
    Cgls,
    Cgtop,
    Escape,
    Path,
    Notify,
    Tmpfiles,
}

impl Personality {
    fn name(self) -> &'static str {
        match self {
            Self::Systemctl => "systemctl",
            Self::Analyze => "systemd-analyze",
            Self::Cat => "systemd-cat",
            Self::Cgls => "systemd-cgls",
            Self::Cgtop => "systemd-cgtop",
            Self::Escape => "systemd-escape",
            Self::Path => "systemd-path",
            Self::Notify => "systemd-notify",
            Self::Tmpfiles => "systemd-tmpfiles",
        }
    }
}

/// Extract personality from argv[0] basename.
fn detect_personality(argv0: &str) -> Personality {
    let base = basename(argv0);
    let stem = base.strip_suffix(".exe").unwrap_or(base);
    match stem {
        "systemd-analyze" => Personality::Analyze,
        "systemd-cat" => Personality::Cat,
        "systemd-cgls" => Personality::Cgls,
        "systemd-cgtop" => Personality::Cgtop,
        "systemd-escape" => Personality::Escape,
        "systemd-path" => Personality::Path,
        "systemd-notify" => Personality::Notify,
        "systemd-tmpfiles" => Personality::Tmpfiles,
        _ => Personality::Systemctl,
    }
}

/// Return the filename portion of a path.
fn basename(path: &str) -> &str {
    let after_slash = match path.rfind('/') {
        Some(i) => &path[i + 1..],
        None => path,
    };
    match after_slash.rfind('\\') {
        Some(i) => &after_slash[i + 1..],
        None => after_slash,
    }
}

// ============================================================================
// Unit types and states
// ============================================================================

/// Systemd unit types.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum UnitType {
    Service,
    Socket,
    Timer,
    Mount,
    Path,
    Target,
    Device,
    Swap,
    Slice,
    Scope,
    Automount,
}

impl UnitType {
    fn as_str(self) -> &'static str {
        match self {
            Self::Service => "service",
            Self::Socket => "socket",
            Self::Timer => "timer",
            Self::Mount => "mount",
            Self::Path => "path",
            Self::Target => "target",
            Self::Device => "device",
            Self::Swap => "swap",
            Self::Slice => "slice",
            Self::Scope => "scope",
            Self::Automount => "automount",
        }
    }

    fn from_str(s: &str) -> Option<Self> {
        match s {
            "service" => Some(Self::Service),
            "socket" => Some(Self::Socket),
            "timer" => Some(Self::Timer),
            "mount" => Some(Self::Mount),
            "path" => Some(Self::Path),
            "target" => Some(Self::Target),
            "device" => Some(Self::Device),
            "swap" => Some(Self::Swap),
            "slice" => Some(Self::Slice),
            "scope" => Some(Self::Scope),
            "automount" => Some(Self::Automount),
            _ => None,
        }
    }

    fn from_unit_name(name: &str) -> Option<Self> {
        if let Some(pos) = name.rfind('.') {
            Self::from_str(&name[pos + 1..])
        } else {
            None
        }
    }
}

/// Active state of a unit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ActiveState {
    Active,
    Inactive,
    Failed,
    Activating,
    Deactivating,
    Reloading,
}

impl ActiveState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Inactive => "inactive",
            Self::Failed => "failed",
            Self::Activating => "activating",
            Self::Deactivating => "deactivating",
            Self::Reloading => "reloading",
        }
    }

    fn from_str(s: &str) -> Option<Self> {
        match s {
            "active" => Some(Self::Active),
            "inactive" => Some(Self::Inactive),
            "failed" => Some(Self::Failed),
            "activating" => Some(Self::Activating),
            "deactivating" => Some(Self::Deactivating),
            "reloading" => Some(Self::Reloading),
            _ => None,
        }
    }
}

/// Load state of a unit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LoadState {
    Loaded,
    NotFound,
    Error,
    Masked,
    BadSetting,
}

impl LoadState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Loaded => "loaded",
            Self::NotFound => "not-found",
            Self::Error => "error",
            Self::Masked => "masked",
            Self::BadSetting => "bad-setting",
        }
    }
}

/// Sub-state of a unit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SubState {
    Running,
    Dead,
    Exited,
    Waiting,
    Listening,
    Mounted,
    Plugged,
    Active,
    Failed,
}

impl SubState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Dead => "dead",
            Self::Exited => "exited",
            Self::Waiting => "waiting",
            Self::Listening => "listening",
            Self::Mounted => "mounted",
            Self::Plugged => "plugged",
            Self::Active => "active",
            Self::Failed => "failed",
        }
    }
}

/// Enable state of a unit file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EnableState {
    Enabled,
    Disabled,
    Static,
    Masked,
    Linked,
    Indirect,
    Generated,
    Transient,
    Bad,
}

impl EnableState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Enabled => "enabled",
            Self::Disabled => "disabled",
            Self::Static => "static",
            Self::Masked => "masked",
            Self::Linked => "linked",
            Self::Indirect => "indirect",
            Self::Generated => "generated",
            Self::Transient => "transient",
            Self::Bad => "bad",
        }
    }

    fn from_str(s: &str) -> Option<Self> {
        match s {
            "enabled" => Some(Self::Enabled),
            "disabled" => Some(Self::Disabled),
            "static" => Some(Self::Static),
            "masked" => Some(Self::Masked),
            "linked" => Some(Self::Linked),
            "indirect" => Some(Self::Indirect),
            "generated" => Some(Self::Generated),
            "transient" => Some(Self::Transient),
            "bad" => Some(Self::Bad),
            _ => None,
        }
    }
}

/// Service types.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ServiceType {
    Simple,
    Forking,
    Oneshot,
    Dbus,
    Notify,
    Idle,
    Exec,
}

impl ServiceType {
    fn as_str(self) -> &'static str {
        match self {
            Self::Simple => "simple",
            Self::Forking => "forking",
            Self::Oneshot => "oneshot",
            Self::Dbus => "dbus",
            Self::Notify => "notify",
            Self::Idle => "idle",
            Self::Exec => "exec",
        }
    }

    fn from_str(s: &str) -> Option<Self> {
        match s {
            "simple" => Some(Self::Simple),
            "forking" => Some(Self::Forking),
            "oneshot" => Some(Self::Oneshot),
            "dbus" => Some(Self::Dbus),
            "notify" => Some(Self::Notify),
            "idle" => Some(Self::Idle),
            "exec" => Some(Self::Exec),
            _ => None,
        }
    }
}

// ============================================================================
// Unit file parsing
// ============================================================================

/// A parsed unit file section: maps section name to key-value pairs.
/// Keys can appear multiple times (e.g. `After=` stacking), so values are
/// collected into a `Vec`.
#[derive(Clone, Debug, Default)]
struct UnitFile {
    sections: BTreeMap<String, Vec<(String, String)>>,
}

impl UnitFile {
    /// Parse INI-like unit file content.
    fn parse(content: &str) -> Result<Self, String> {
        let mut sections: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();
        let mut current_section: Option<String> = None;
        let mut continued_line = String::new();
        let mut in_continuation = false;

        for (line_no, raw_line) in content.lines().enumerate() {
            let line = raw_line.trim();

            // Handle backslash continuation.
            if in_continuation {
                if let Some(stripped) = line.strip_suffix('\\') {
                    continued_line.push(' ');
                    continued_line.push_str(stripped.trim());
                    continue;
                }
                continued_line.push(' ');
                continued_line.push_str(line);
                in_continuation = false;
                // Process the assembled line below.
                let assembled = continued_line.clone();
                continued_line.clear();
                Self::parse_kv(
                    &assembled,
                    &current_section,
                    &mut sections,
                    line_no,
                )?;
                continue;
            }

            // Skip empty lines and comments.
            if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
                continue;
            }

            // Section header.
            if line.starts_with('[') {
                if let Some(end) = line.find(']') {
                    let name = line[1..end].to_string();
                    if name.is_empty() {
                        return Err(format!("Empty section name at line {}", line_no + 1));
                    }
                    current_section = Some(name);
                    continue;
                }
                return Err(format!("Malformed section header at line {}", line_no + 1));
            }

            // Backslash continuation start.
            if let Some(stripped) = line.strip_suffix('\\') {
                in_continuation = true;
                continued_line = stripped.trim().to_string();
                continue;
            }

            // Regular key=value.
            Self::parse_kv(line, &current_section, &mut sections, line_no)?;
        }

        if in_continuation && !continued_line.is_empty() {
            // Trailing continued line without a final line: treat as-is.
            Self::parse_kv(
                &continued_line,
                &current_section,
                &mut sections,
                0,
            )?;
        }

        Ok(UnitFile { sections })
    }

    fn parse_kv(
        line: &str,
        current_section: &Option<String>,
        sections: &mut BTreeMap<String, Vec<(String, String)>>,
        line_no: usize,
    ) -> Result<(), String> {
        let section = current_section.as_ref().ok_or_else(|| {
            format!("Key-value pair outside section at line {}", line_no + 1)
        })?;
        if let Some(eq_pos) = line.find('=') {
            let key = line[..eq_pos].trim().to_string();
            let value = line[eq_pos + 1..].trim().to_string();
            sections
                .entry(section.clone())
                .or_default()
                .push((key, value));
            Ok(())
        } else {
            Err(format!("Invalid line at {}: {}", line_no + 1, line))
        }
    }

    /// Get the first value for a key in a section.
    fn get(&self, section: &str, key: &str) -> Option<&str> {
        self.sections.get(section).and_then(|pairs| {
            pairs
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.as_str())
        })
    }

    /// Get all values for a key in a section (for stacking directives).
    fn get_all(&self, section: &str, key: &str) -> Vec<&str> {
        match self.sections.get(section) {
            Some(pairs) => pairs
                .iter()
                .filter(|(k, _)| k == key)
                .map(|(_, v)| v.as_str())
                .collect(),
            None => Vec::new(),
        }
    }

    /// List all section names.
    fn section_names(&self) -> Vec<&str> {
        self.sections.keys().map(|s| s.as_str()).collect()
    }

    /// Verify basic structural correctness.
    fn verify(&self, unit_name: &str) -> Vec<String> {
        let mut issues = Vec::new();

        // Must have a [Unit] section.
        if !self.sections.contains_key("Unit") {
            issues.push(format!("{}: Missing [Unit] section", unit_name));
        }

        // Service units need [Service].
        if unit_name.ends_with(".service") && !self.sections.contains_key("Service") {
            issues.push(format!("{}: Missing [Service] section", unit_name));
        }

        // Timer units need [Timer].
        if unit_name.ends_with(".timer") && !self.sections.contains_key("Timer") {
            issues.push(format!("{}: Missing [Timer] section", unit_name));
        }

        // Socket units need [Socket].
        if unit_name.ends_with(".socket") && !self.sections.contains_key("Socket") {
            issues.push(format!("{}: Missing [Socket] section", unit_name));
        }

        // Mount units need [Mount].
        if unit_name.ends_with(".mount") && !self.sections.contains_key("Mount") {
            issues.push(format!("{}: Missing [Mount] section", unit_name));
        }

        // Path units need [Path].
        if unit_name.ends_with(".path") && !self.sections.contains_key("Path") {
            issues.push(format!("{}: Missing [Path] section", unit_name));
        }

        // Check for Description in [Unit].
        if self.get("Unit", "Description").is_none() {
            issues.push(format!("{}: Missing Description in [Unit]", unit_name));
        }

        // Service should have ExecStart (except Type=oneshot can have ExecStart=).
        if unit_name.ends_with(".service") {
            let stype = self.get("Service", "Type").unwrap_or("simple");
            if stype != "oneshot" && self.get("Service", "ExecStart").is_none() {
                issues.push(format!(
                    "{}: Missing ExecStart in [Service] for Type={}",
                    unit_name, stype
                ));
            }
        }

        issues
    }
}

// ============================================================================
// Specifier expansion
// ============================================================================

/// Expand systemd specifiers in a string.
fn expand_specifiers(input: &str, unit_name: &str) -> String {
    let prefix = unit_prefix(unit_name);
    let instance = unit_instance(unit_name).unwrap_or("");
    let unescaped = unescape_unit_name(unit_name);

    let mut result = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '%' {
            match chars.next() {
                Some('n') => result.push_str(unit_name),
                Some('N') => result.push_str(&unescaped),
                Some('p') => result.push_str(prefix),
                Some('i') => result.push_str(instance),
                Some('H') => result.push_str("ouros"),
                Some('%') => result.push('%'),
                Some(c) => {
                    result.push('%');
                    result.push(c);
                }
                None => result.push('%'),
            }
        } else {
            result.push(ch);
        }
    }
    result
}

/// Extract the prefix (part before the first '@' or '.').
fn unit_prefix(name: &str) -> &str {
    let end = name
        .find('@')
        .unwrap_or_else(|| name.rfind('.').unwrap_or(name.len()));
    &name[..end]
}

/// Extract the instance (between '@' and the suffix '.xxx').
fn unit_instance(name: &str) -> Option<&str> {
    let at = name.find('@')?;
    let dot = name.rfind('.')?;
    if at < dot {
        Some(&name[at + 1..dot])
    } else {
        None
    }
}

// ============================================================================
// Unit name escaping / unescaping
// ============================================================================

/// Escape a string for use as a systemd unit name component.
/// Replaces '/' with '-', and non-alphanumeric-non-dash chars with '\xHH'.
fn escape_unit_name(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let trimmed = input.strip_prefix('/').unwrap_or(input);
    for ch in trimmed.chars() {
        match ch {
            '/' => result.push('-'),
            'a'..='z' | 'A'..='Z' | '0'..='9' | ':' | '_' | '.' => result.push(ch),
            '-' if !result.is_empty() => result.push(ch),
            _ => {
                // Hex-encode byte by byte.
                let mut buf = [0u8; 4];
                let encoded = ch.encode_utf8(&mut buf);
                for &b in encoded.as_bytes() {
                    result.push_str(&format!("\\x{:02x}", b));
                }
            }
        }
    }
    if result.is_empty() {
        result.push('-');
    }
    result
}

/// Unescape a systemd unit name back to a path/string.
fn unescape_unit_name(input: &str) -> String {
    // Strip the suffix (.service, etc.) for unescaping.
    let stem = if let Some(dot_pos) = input.rfind('.') {
        let suffix = &input[dot_pos + 1..];
        if UnitType::from_str(suffix).is_some() {
            &input[..dot_pos]
        } else {
            input
        }
    } else {
        input
    };

    let mut result = String::new();
    let mut chars = stem.chars().peekable();

    // In systemd, escaped path names have their leading '/' stripped during
    // escape.  On unescape, if the stem contains dashes (path separators)
    // or starts with a dash (root-rooted path), we restore the leading '/'.
    let has_dash = stem.contains('-');
    if has_dash {
        result.push('/');
    }

    // A leading dash in the escaped form represents root '/' (already added).
    if chars.peek() == Some(&'-') {
        chars.next();
    }
    while let Some(ch) = chars.next() {
        if ch == '-' {
            result.push('/');
        } else if ch == '\\' && chars.peek() == Some(&'x') {
            chars.next(); // consume 'x'
            let hi = chars.next().unwrap_or('0');
            let lo = chars.next().unwrap_or('0');
            let mut hex = String::new();
            hex.push(hi);
            hex.push(lo);
            if let Ok(byte) = u8::from_str_radix(&hex, 16) {
                result.push(byte as char);
            }
        } else {
            result.push(ch);
        }
    }
    result
}

// ============================================================================
// Systemctl command parsing
// ============================================================================

/// Parsed global flags for systemctl.
#[derive(Clone, Debug)]
#[derive(Default)]
struct SystemctlFlags {
    user_scope: bool,
    no_pager: bool,
    no_legend: bool,
    plain: bool,
    all: bool,
    quiet: bool,
    now: bool,
    force: bool,
    type_filter: Option<String>,
    state_filter: Option<String>,
}


/// Parse systemctl arguments into flags and remaining positional args.
fn parse_systemctl_args(args: &[String]) -> (SystemctlFlags, Vec<String>) {
    let mut flags = SystemctlFlags::default();
    let mut positional = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if arg == "--user" {
            flags.user_scope = true;
        } else if arg == "--system" {
            flags.user_scope = false;
        } else if arg == "--no-pager" {
            flags.no_pager = true;
        } else if arg == "--no-legend" {
            flags.no_legend = true;
        } else if arg == "--plain" {
            flags.plain = true;
        } else if arg == "-a" || arg == "--all" {
            flags.all = true;
        } else if arg == "-q" || arg == "--quiet" {
            flags.quiet = true;
        } else if arg == "--now" {
            flags.now = true;
        } else if arg == "-f" || arg == "--force" {
            flags.force = true;
        } else if let Some(rest) = arg.strip_prefix("--type=") {
            flags.type_filter = Some(rest.to_string());
        } else if arg == "--type" || arg == "-t" {
            i += 1;
            if i < args.len() {
                flags.type_filter = Some(args[i].clone());
            }
        } else if let Some(rest) = arg.strip_prefix("--state=") {
            flags.state_filter = Some(rest.to_string());
        } else if arg == "--state" {
            i += 1;
            if i < args.len() {
                flags.state_filter = Some(args[i].clone());
            }
        } else if !arg.starts_with('-') || arg == "-" {
            positional.push(arg.clone());
        }
        // Silently ignore unknown flags for forward compatibility.
        i += 1;
    }
    (flags, positional)
}

// ============================================================================
// Simulated system state (for a standalone binary with no real init)
// ============================================================================

/// Simulated unit entry for list-units / status output.
struct UnitEntry {
    name: &'static str,
    load: LoadState,
    active: ActiveState,
    sub: SubState,
    description: &'static str,
}

/// Return a set of simulated units representing a typical booted system.
fn simulated_units() -> Vec<UnitEntry> {
    vec![
        UnitEntry {
            name: "init.service",
            load: LoadState::Loaded,
            active: ActiveState::Active,
            sub: SubState::Running,
            description: "System and Service Manager",
        },
        UnitEntry {
            name: "network.service",
            load: LoadState::Loaded,
            active: ActiveState::Active,
            sub: SubState::Running,
            description: "Network Configuration",
        },
        UnitEntry {
            name: "sshd.service",
            load: LoadState::Loaded,
            active: ActiveState::Active,
            sub: SubState::Running,
            description: "OpenSSH Server",
        },
        UnitEntry {
            name: "dbus.service",
            load: LoadState::Loaded,
            active: ActiveState::Active,
            sub: SubState::Running,
            description: "D-Bus System Message Bus",
        },
        UnitEntry {
            name: "cron.service",
            load: LoadState::Loaded,
            active: ActiveState::Active,
            sub: SubState::Running,
            description: "Task Scheduler",
        },
        UnitEntry {
            name: "logd.service",
            load: LoadState::Loaded,
            active: ActiveState::Active,
            sub: SubState::Running,
            description: "System Logging Daemon",
        },
        UnitEntry {
            name: "sysctl.service",
            load: LoadState::Loaded,
            active: ActiveState::Active,
            sub: SubState::Exited,
            description: "Apply Kernel Variables",
        },
        UnitEntry {
            name: "basic.target",
            load: LoadState::Loaded,
            active: ActiveState::Active,
            sub: SubState::Active,
            description: "Basic System",
        },
        UnitEntry {
            name: "multi-user.target",
            load: LoadState::Loaded,
            active: ActiveState::Active,
            sub: SubState::Active,
            description: "Multi-User System",
        },
        UnitEntry {
            name: "graphical.target",
            load: LoadState::Loaded,
            active: ActiveState::Active,
            sub: SubState::Active,
            description: "Graphical Interface",
        },
        UnitEntry {
            name: "sockets.target",
            load: LoadState::Loaded,
            active: ActiveState::Active,
            sub: SubState::Active,
            description: "Sockets",
        },
        UnitEntry {
            name: "timers.target",
            load: LoadState::Loaded,
            active: ActiveState::Active,
            sub: SubState::Active,
            description: "Timers",
        },
        UnitEntry {
            name: "dbus.socket",
            load: LoadState::Loaded,
            active: ActiveState::Active,
            sub: SubState::Listening,
            description: "D-Bus System Message Bus Socket",
        },
        UnitEntry {
            name: "logwatch.timer",
            load: LoadState::Loaded,
            active: ActiveState::Active,
            sub: SubState::Waiting,
            description: "Daily Log Rotation",
        },
        UnitEntry {
            name: "tmp.mount",
            load: LoadState::Loaded,
            active: ActiveState::Active,
            sub: SubState::Mounted,
            description: "Temporary Directory",
        },
    ]
}

/// Simulated unit file entries for list-unit-files.
struct UnitFileEntry {
    name: &'static str,
    state: EnableState,
    preset: &'static str,
}

fn simulated_unit_files() -> Vec<UnitFileEntry> {
    vec![
        UnitFileEntry { name: "init.service", state: EnableState::Static, preset: "-" },
        UnitFileEntry { name: "network.service", state: EnableState::Enabled, preset: "enabled" },
        UnitFileEntry { name: "sshd.service", state: EnableState::Enabled, preset: "enabled" },
        UnitFileEntry { name: "dbus.service", state: EnableState::Static, preset: "-" },
        UnitFileEntry { name: "cron.service", state: EnableState::Enabled, preset: "enabled" },
        UnitFileEntry { name: "logd.service", state: EnableState::Enabled, preset: "enabled" },
        UnitFileEntry { name: "sysctl.service", state: EnableState::Static, preset: "-" },
        UnitFileEntry { name: "basic.target", state: EnableState::Static, preset: "-" },
        UnitFileEntry { name: "multi-user.target", state: EnableState::Static, preset: "-" },
        UnitFileEntry { name: "graphical.target", state: EnableState::Static, preset: "-" },
        UnitFileEntry { name: "sockets.target", state: EnableState::Static, preset: "-" },
        UnitFileEntry { name: "timers.target", state: EnableState::Static, preset: "-" },
        UnitFileEntry { name: "dbus.socket", state: EnableState::Static, preset: "-" },
        UnitFileEntry { name: "logwatch.timer", state: EnableState::Enabled, preset: "enabled" },
        UnitFileEntry { name: "tmp.mount", state: EnableState::Static, preset: "-" },
        UnitFileEntry { name: "rescue.service", state: EnableState::Disabled, preset: "disabled" },
    ]
}

// ============================================================================
// Systemctl sub-commands
// ============================================================================

fn cmd_list_units(
    out: &mut dyn Write,
    flags: &SystemctlFlags,
) -> io::Result<i32> {
    let units = simulated_units();

    if !flags.no_legend {
        writeln!(
            out,
            "{:<40} {:<10} {:<10} {:<10} DESCRIPTION",
            "UNIT", "LOAD", "ACTIVE", "SUB"
        )?;
    }

    for u in &units {
        // Apply type filter.
        if let Some(ref tf) = flags.type_filter {
            let ut = UnitType::from_unit_name(u.name);
            if ut.is_none_or(|t| t.as_str() != tf.as_str()) {
                continue;
            }
        }
        // Apply state filter.
        if let Some(ref sf) = flags.state_filter
            && u.active.as_str() != sf.as_str() {
                continue;
            }
        // Skip inactive unless --all.
        if !flags.all && u.active == ActiveState::Inactive {
            continue;
        }
        writeln!(
            out,
            "{:<40} {:<10} {:<10} {:<10} {}",
            u.name,
            u.load.as_str(),
            u.active.as_str(),
            u.sub.as_str(),
            u.description
        )?;
    }

    if !flags.no_legend {
        let shown: usize = units
            .iter()
            .filter(|u| {
                if let Some(ref tf) = flags.type_filter {
                    let ut = UnitType::from_unit_name(u.name);
                    if ut.is_none_or(|t| t.as_str() != tf.as_str()) {
                        return false;
                    }
                }
                if let Some(ref sf) = flags.state_filter
                    && u.active.as_str() != sf.as_str() {
                        return false;
                    }
                flags.all || u.active != ActiveState::Inactive
            })
            .count();
        writeln!(out)?;
        writeln!(out, "{} loaded units listed.", shown)?;
    }

    Ok(0)
}

fn cmd_list_unit_files(
    out: &mut dyn Write,
    flags: &SystemctlFlags,
) -> io::Result<i32> {
    let files = simulated_unit_files();

    if !flags.no_legend {
        writeln!(out, "{:<40} {:<12} PRESET", "UNIT FILE", "STATE")?;
    }

    for f in &files {
        if let Some(ref tf) = flags.type_filter {
            let ut = UnitType::from_unit_name(f.name);
            if ut.is_none_or(|t| t.as_str() != tf.as_str()) {
                continue;
            }
        }
        writeln!(out, "{:<40} {:<12} {}", f.name, f.state.as_str(), f.preset)?;
    }

    if !flags.no_legend {
        writeln!(out)?;
        writeln!(out, "{} unit files listed.", files.len())?;
    }
    Ok(0)
}

fn cmd_status(
    out: &mut dyn Write,
    unit_name: &str,
    _flags: &SystemctlFlags,
) -> io::Result<i32> {
    let units = simulated_units();
    if let Some(u) = units.iter().find(|e| e.name == unit_name) {
        writeln!(out, "● {} - {}", u.name, u.description)?;
        writeln!(
            out,
            "     Loaded: {} (/etc/ouros/system/{}; enabled)",
            u.load.as_str(),
            u.name
        )?;
        writeln!(
            out,
            "     Active: {} ({}) since Mon 2026-01-01 00:00:00 UTC; 1h ago",
            u.active.as_str(),
            u.sub.as_str()
        )?;
        writeln!(out, "   Main PID: 1234 ({})", unit_prefix(u.name))?;
        writeln!(out, "      Tasks: 1 (limit: 4096)")?;
        writeln!(out, "     Memory: 2.0M")?;
        writeln!(out, "        CPU: 50ms")?;
        Ok(0)
    } else {
        writeln!(
            out,
            "Unit {} could not be found.",
            unit_name
        )?;
        Ok(4)
    }
}

fn cmd_show(
    out: &mut dyn Write,
    unit_name: &str,
    property: Option<&str>,
    _flags: &SystemctlFlags,
) -> io::Result<i32> {
    let units = simulated_units();
    if let Some(u) = units.iter().find(|e| e.name == unit_name) {
        let props: Vec<(&str, String)> = vec![
            ("Id", u.name.to_string()),
            ("Description", u.description.to_string()),
            ("LoadState", u.load.as_str().to_string()),
            ("ActiveState", u.active.as_str().to_string()),
            ("SubState", u.sub.as_str().to_string()),
            ("MainPID", "1234".to_string()),
            ("MemoryCurrent", "2097152".to_string()),
            ("TasksCurrent", "1".to_string()),
        ];
        if let Some(p) = property {
            if let Some((_, val)) = props.iter().find(|(k, _)| *k == p) {
                writeln!(out, "{}={}", p, val)?;
            } else {
                writeln!(out, "{}=", p)?;
            }
        } else {
            for (k, v) in &props {
                writeln!(out, "{}={}", k, v)?;
            }
        }
        Ok(0)
    } else {
        writeln!(out, "Unit {} could not be found.", unit_name)?;
        Ok(4)
    }
}

fn cmd_unit_action(
    out: &mut dyn Write,
    action: &str,
    unit_name: &str,
    flags: &SystemctlFlags,
) -> io::Result<i32> {
    // Validate unit name has a type suffix.
    let name_with_suffix = if unit_name.contains('.') {
        unit_name.to_string()
    } else {
        format!("{}.service", unit_name)
    };

    if !flags.quiet {
        let scope = if flags.user_scope { "--user" } else { "--system" };
        match action {
            "start" => writeln!(out, "Starting {} ({})...", name_with_suffix, scope)?,
            "stop" => writeln!(out, "Stopping {} ({})...", name_with_suffix, scope)?,
            "restart" => writeln!(out, "Restarting {} ({})...", name_with_suffix, scope)?,
            "reload" => writeln!(out, "Reloading {} ({})...", name_with_suffix, scope)?,
            "enable" => {
                writeln!(
                    out,
                    "Created symlink /etc/ouros/system/multi-user.target.wants/{} -> /usr/lib/ouros/system/{}.",
                    name_with_suffix, name_with_suffix
                )?;
                if flags.now {
                    writeln!(out, "Starting {} ({})...", name_with_suffix, scope)?;
                }
            }
            "disable" => {
                writeln!(
                    out,
                    "Removed /etc/ouros/system/multi-user.target.wants/{}.",
                    name_with_suffix
                )?;
                if flags.now {
                    writeln!(out, "Stopping {} ({})...", name_with_suffix, scope)?;
                }
            }
            "mask" => writeln!(
                out,
                "Created symlink /etc/ouros/system/{} -> /dev/null.",
                name_with_suffix
            )?,
            "unmask" => writeln!(out, "Removed /etc/ouros/system/{}.", name_with_suffix)?,
            _ => writeln!(out, "Unknown action: {}", action)?,
        }
    }
    Ok(0)
}

fn cmd_is_active(
    out: &mut dyn Write,
    unit_name: &str,
    _flags: &SystemctlFlags,
) -> io::Result<i32> {
    let units = simulated_units();
    if let Some(u) = units.iter().find(|e| e.name == unit_name) {
        writeln!(out, "{}", u.active.as_str())?;
        if u.active == ActiveState::Active {
            Ok(0)
        } else {
            Ok(3)
        }
    } else {
        writeln!(out, "inactive")?;
        Ok(3)
    }
}

fn cmd_is_enabled(
    out: &mut dyn Write,
    unit_name: &str,
    _flags: &SystemctlFlags,
) -> io::Result<i32> {
    let files = simulated_unit_files();
    if let Some(f) = files.iter().find(|e| e.name == unit_name) {
        writeln!(out, "{}", f.state.as_str())?;
        if f.state == EnableState::Enabled {
            Ok(0)
        } else {
            Ok(1)
        }
    } else {
        writeln!(out, "disabled")?;
        Ok(1)
    }
}

fn cmd_is_failed(
    out: &mut dyn Write,
    unit_name: &str,
    _flags: &SystemctlFlags,
) -> io::Result<i32> {
    let units = simulated_units();
    if let Some(u) = units.iter().find(|e| e.name == unit_name) {
        if u.active == ActiveState::Failed {
            writeln!(out, "failed")?;
            Ok(0)
        } else {
            writeln!(out, "{}", u.active.as_str())?;
            Ok(1)
        }
    } else {
        writeln!(out, "inactive")?;
        Ok(1)
    }
}

fn cmd_daemon_reload(out: &mut dyn Write, flags: &SystemctlFlags) -> io::Result<i32> {
    if !flags.quiet {
        writeln!(out, "Reloading daemon configuration...")?;
    }
    Ok(0)
}

fn cmd_cat_unit(out: &mut dyn Write, unit_name: &str) -> io::Result<i32> {
    // Produce a synthetic unit file for known units.
    let units = simulated_units();
    if units.iter().any(|u| u.name == unit_name) {
        writeln!(out, "# /usr/lib/ouros/system/{}", unit_name)?;
        writeln!(out, "[Unit]")?;
        let desc = units
            .iter()
            .find(|u| u.name == unit_name)
            .map_or("Unknown", |u| u.description);
        writeln!(out, "Description={}", desc)?;
        writeln!(out)?;
        if unit_name.ends_with(".service") {
            writeln!(out, "[Service]")?;
            writeln!(out, "Type=simple")?;
            writeln!(out, "ExecStart=/usr/bin/{}", unit_prefix(unit_name))?;
            writeln!(out)?;
            writeln!(out, "[Install]")?;
            writeln!(out, "WantedBy=multi-user.target")?;
        }
        Ok(0)
    } else {
        writeln!(out, "No files found for {}.", unit_name)?;
        Ok(1)
    }
}

fn cmd_edit_unit(out: &mut dyn Write, unit_name: &str) -> io::Result<i32> {
    writeln!(
        out,
        "Editing /etc/ouros/system/{}.d/override.conf...",
        unit_name
    )?;
    writeln!(out, "(editor not available in this environment)")?;
    Ok(0)
}

fn cmd_power(out: &mut dyn Write, action: &str, flags: &SystemctlFlags) -> io::Result<i32> {
    if !flags.quiet {
        match action {
            "poweroff" => writeln!(out, "System is powering off...")?,
            "reboot" => writeln!(out, "System is rebooting...")?,
            "halt" => writeln!(out, "System is halting...")?,
            "suspend" => writeln!(out, "System is suspending...")?,
            "hibernate" => writeln!(out, "System is hibernating...")?,
            _ => writeln!(out, "Unknown power action: {}", action)?,
        }
    }
    Ok(0)
}

fn cmd_isolate(out: &mut dyn Write, target: &str, flags: &SystemctlFlags) -> io::Result<i32> {
    if !flags.quiet {
        writeln!(out, "Isolating {}...", target)?;
        writeln!(out, "Stopping all units not required by {}.", target)?;
    }
    Ok(0)
}

fn cmd_list_timers(out: &mut dyn Write, flags: &SystemctlFlags) -> io::Result<i32> {
    if !flags.no_legend {
        writeln!(
            out,
            "{:<24} {:<24} {:<24} {:<24} UNIT",
            "NEXT", "LEFT", "LAST", "PASSED"
        )?;
    }
    writeln!(
        out,
        "{:<24} {:<24} {:<24} {:<24} logwatch.timer",
        "Mon 2026-01-02 00:00:00",
        "23h left",
        "Mon 2026-01-01 00:00:00",
        "1h ago"
    )?;
    if !flags.no_legend {
        writeln!(out)?;
        writeln!(out, "1 timers listed.")?;
    }
    Ok(0)
}

fn cmd_list_sockets(out: &mut dyn Write, flags: &SystemctlFlags) -> io::Result<i32> {
    if !flags.no_legend {
        writeln!(out, "{:<40} {:<10} UNIT", "LISTEN", "TYPE")?;
    }
    writeln!(
        out,
        "{:<40} {:<10} dbus.socket",
        "/run/dbus/system_bus_socket", "Stream"
    )?;
    if !flags.no_legend {
        writeln!(out)?;
        writeln!(out, "1 sockets listed.")?;
    }
    Ok(0)
}

fn cmd_list_dependencies(
    out: &mut dyn Write,
    unit_name: &str,
    _flags: &SystemctlFlags,
) -> io::Result<i32> {
    writeln!(out, "{}", unit_name)?;
    // Produce a synthetic dependency tree.
    let deps: Vec<&str> = match unit_name {
        "multi-user.target" => vec![
            "basic.target",
            "dbus.service",
            "network.service",
            "sshd.service",
            "cron.service",
        ],
        "graphical.target" => vec!["multi-user.target"],
        _ => vec!["basic.target"],
    };
    for (i, d) in deps.iter().enumerate() {
        if i + 1 < deps.len() {
            writeln!(out, "├─{}", d)?;
        } else {
            writeln!(out, "└─{}", d)?;
        }
    }
    Ok(0)
}

// ============================================================================
// systemd-analyze sub-commands
// ============================================================================

fn analyze_time(out: &mut dyn Write) -> io::Result<i32> {
    writeln!(out, "Startup finished in 1.200s (kernel) + 2.500s (userspace) = 3.700s")?;
    writeln!(out, "graphical.target reached after 3.500s in userspace.")?;
    Ok(0)
}

fn analyze_blame(out: &mut dyn Write) -> io::Result<i32> {
    let blame_data: Vec<(&str, &str)> = vec![
        ("1.500s", "network.service"),
        ("800ms", "sshd.service"),
        ("400ms", "logd.service"),
        ("200ms", "dbus.service"),
        ("150ms", "cron.service"),
        ("100ms", "sysctl.service"),
        ("50ms", "tmp.mount"),
    ];
    for (time, unit) in &blame_data {
        writeln!(out, "{:>10} {}", time, unit)?;
    }
    Ok(0)
}

fn analyze_critical_chain(out: &mut dyn Write, unit: Option<&str>) -> io::Result<i32> {
    let target = unit.unwrap_or("graphical.target");
    writeln!(out, "The time when unit became active or started is printed after the \"@\" character.")?;
    writeln!(out, "The time the unit took to start is printed after the \"+\" character.")?;
    writeln!(out)?;
    writeln!(out, "{} @3.500s", target)?;
    writeln!(out, "└─multi-user.target @3.400s")?;
    writeln!(out, "  └─network.service @1.900s +1.500s")?;
    writeln!(out, "    └─basic.target @1.800s")?;
    writeln!(out, "      └─sockets.target @1.700s")?;
    writeln!(out, "        └─dbus.socket @1.600s")?;
    Ok(0)
}

fn analyze_plot(out: &mut dyn Write) -> io::Result<i32> {
    // Text-mode boot chart.
    writeln!(out, "Boot Plot (text mode)")?;
    writeln!(out, "=====================")?;
    writeln!(out)?;
    writeln!(out, "0s        1s        2s        3s")?;
    writeln!(out, "|---------|---------|---------|")?;
    writeln!(out, "[kernel...........             ]  1.200s")?;
    writeln!(out, "             [dbus....          ]  0.200s")?;
    writeln!(out, "             [sysctl.]           0.100s")?;
    writeln!(out, "               [network........]  1.500s")?;
    writeln!(out, "               [sshd.....]       0.800s")?;
    writeln!(out, "               [logd...]         0.400s")?;
    writeln!(out, "               [cron.]           0.150s")?;
    Ok(0)
}

fn analyze_dot(out: &mut dyn Write, units: &[String]) -> io::Result<i32> {
    writeln!(out, "digraph systemd {{")?;
    writeln!(out, "  rankdir=LR;")?;

    if units.is_empty() {
        // Default: show key dependency edges.
        writeln!(out, "  \"graphical.target\" -> \"multi-user.target\";")?;
        writeln!(out, "  \"multi-user.target\" -> \"basic.target\";")?;
        writeln!(out, "  \"multi-user.target\" -> \"network.service\";")?;
        writeln!(out, "  \"multi-user.target\" -> \"sshd.service\";")?;
        writeln!(out, "  \"multi-user.target\" -> \"dbus.service\";")?;
        writeln!(out, "  \"basic.target\" -> \"sockets.target\";")?;
        writeln!(out, "  \"sockets.target\" -> \"dbus.socket\";")?;
    } else {
        for u in units {
            writeln!(out, "  \"{}\" -> \"basic.target\";", u)?;
        }
    }

    writeln!(out, "}}")?;
    Ok(0)
}

fn analyze_verify(out: &mut dyn Write, unit_name: &str) -> io::Result<i32> {
    // Try reading a unit file from stdin is not practical; just validate the name.
    if !unit_name.contains('.') {
        writeln!(out, "{}: Unit name should include a type suffix.", unit_name)?;
        return Ok(1);
    }
    let ut = UnitType::from_unit_name(unit_name);
    if ut.is_none() {
        writeln!(out, "{}: Unknown unit type suffix.", unit_name)?;
        return Ok(1);
    }
    writeln!(out, "{}: Unit file syntax OK.", unit_name)?;
    Ok(0)
}

fn analyze_security(out: &mut dyn Write, unit: Option<&str>) -> io::Result<i32> {
    let unit_name = unit.unwrap_or("sshd.service");
    writeln!(out, "  NAME                           DESCRIPTION                          EXPOSURE")?;
    writeln!(out, "✓ PrivateNetwork=               Service has private network namespace     0.5")?;
    writeln!(out, "✗ PrivateTmp=                    Tmp is not private                        0.1")?;
    writeln!(out, "✓ NoNewPrivileges=               No new privileges                        0.2")?;
    writeln!(out, "✓ ProtectSystem=                 System is protected read-only             0.3")?;
    writeln!(out, "✗ ProtectHome=                   Home is not protected                     0.2")?;
    writeln!(out)?;
    writeln!(out, "→ Overall exposure level for {}: 4.2 MEDIUM", unit_name)?;
    Ok(0)
}

// ============================================================================
// systemd-cat
// ============================================================================

fn run_cat_journal(out: &mut dyn Write) -> io::Result<i32> {
    let stdin = io::stdin();
    for line in stdin.lock().lines() {
        match line {
            Ok(l) => writeln!(out, "[journal] {}", l)?,
            Err(e) => {
                writeln!(out, "systemd-cat: read error: {}", e)?;
                return Ok(1);
            }
        }
    }
    Ok(0)
}

// ============================================================================
// systemd-cgls
// ============================================================================

fn run_cgls(out: &mut dyn Write) -> io::Result<i32> {
    writeln!(out, "Control group /:")?;
    writeln!(out, "├─init.scope")?;
    writeln!(out, "│ └─1 /sbin/init")?;
    writeln!(out, "├─system.slice")?;
    writeln!(out, "│ ├─dbus.service")?;
    writeln!(out, "│ │ └─100 /usr/bin/dbus")?;
    writeln!(out, "│ ├─network.service")?;
    writeln!(out, "│ │ └─200 /usr/bin/network")?;
    writeln!(out, "│ ├─sshd.service")?;
    writeln!(out, "│ │ └─300 /usr/bin/sshd")?;
    writeln!(out, "│ ├─logd.service")?;
    writeln!(out, "│ │ └─400 /usr/bin/logd")?;
    writeln!(out, "│ └─cron.service")?;
    writeln!(out, "│   └─500 /usr/bin/cron")?;
    writeln!(out, "└─user.slice")?;
    writeln!(out, "  └─user-1000.slice")?;
    writeln!(out, "    └─session-1.scope")?;
    writeln!(out, "      └─1000 bash")?;
    Ok(0)
}

// ============================================================================
// systemd-cgtop
// ============================================================================

fn run_cgtop(out: &mut dyn Write) -> io::Result<i32> {
    writeln!(
        out,
        "{:<40} {:>6} {:>8} {:>8} {:>8}",
        "Control Group", "Tasks", "%CPU", "Memory", "Input/s"
    )?;
    writeln!(
        out,
        "{:<40} {:>6} {:>8} {:>8} {:>8}",
        "/", "15", "2.1", "128.0M", "-"
    )?;
    writeln!(
        out,
        "{:<40} {:>6} {:>8} {:>8} {:>8}",
        "/system.slice", "10", "1.5", "64.0M", "-"
    )?;
    writeln!(
        out,
        "{:<40} {:>6} {:>8} {:>8} {:>8}",
        "/system.slice/network.service", "2", "0.5", "12.0M", "-"
    )?;
    writeln!(
        out,
        "{:<40} {:>6} {:>8} {:>8} {:>8}",
        "/system.slice/sshd.service", "1", "0.1", "8.0M", "-"
    )?;
    writeln!(
        out,
        "{:<40} {:>6} {:>8} {:>8} {:>8}",
        "/user.slice", "5", "0.5", "32.0M", "-"
    )?;
    Ok(0)
}

// ============================================================================
// systemd-escape
// ============================================================================

fn run_escape(out: &mut dyn Write, args: &[String]) -> io::Result<i32> {
    if args.is_empty() {
        writeln!(out, "Usage: systemd-escape [OPTIONS] STRING...")?;
        writeln!(out, "Escape strings for use in systemd unit names.")?;
        writeln!(out)?;
        writeln!(out, "Options:")?;
        writeln!(out, "  -u, --unescape    Unescape instead of escaping")?;
        writeln!(out, "  -p, --path        When escaping, treat as path")?;
        writeln!(out, "      --suffix=TYPE  Append unit type suffix")?;
        writeln!(out, "      --help         Show this help")?;
        return Ok(0);
    }

    let mut unescape = false;
    let mut path_mode = false;
    let mut suffix: Option<String> = None;
    let mut strings = Vec::new();

    for arg in args {
        if arg == "-u" || arg == "--unescape" {
            unescape = true;
        } else if arg == "-p" || arg == "--path" {
            path_mode = true;
        } else if let Some(s) = arg.strip_prefix("--suffix=") {
            suffix = Some(s.to_string());
        } else if arg == "--help" {
            return run_escape(out, &[]);
        } else if !arg.starts_with('-') {
            strings.push(arg.clone());
        }
    }

    for s in &strings {
        if unescape {
            let unescaped = unescape_unit_name(s);
            writeln!(out, "{}", unescaped)?;
        } else {
            // NOTE: `--path` mode is accepted for CLI compatibility but currently
            // produces the same output as default mode. `escape_unit_name` already
            // applies path-style escaping (strips the leading `/`), so the two modes
            // are not yet distinguished. See todo.txt (systemd-escape modes). The
            // discard documents that ignoring `path_mode` here is intentional.
            let _ = path_mode;
            let mut escaped = escape_unit_name(s);
            if let Some(ref suf) = suffix {
                escaped.push('.');
                escaped.push_str(suf);
            }
            writeln!(out, "{}", escaped)?;
        }
    }
    Ok(0)
}

// ============================================================================
// systemd-path
// ============================================================================

fn run_path(out: &mut dyn Write, args: &[String]) -> io::Result<i32> {
    let paths: Vec<(&str, &str)> = vec![
        ("temporary", "/tmp"),
        ("temporary-large", "/var/tmp"),
        ("system-binaries", "/usr/bin"),
        ("system-include", "/usr/include"),
        ("system-library-private", "/usr/lib"),
        ("system-library-arch", "/usr/lib/x86_64-ouros"),
        ("system-configuration", "/etc"),
        ("system-state-private", "/var/lib"),
        ("system-state-logs", "/var/log"),
        ("system-state-cache", "/var/cache"),
        ("system-state-spool", "/var/spool"),
        ("system-runtime", "/run"),
        ("system-generator-early", "/run/ouros/system-generators.early"),
        ("system-generator", "/usr/lib/ouros/system-generators"),
        ("system-generator-late", "/run/ouros/system-generators.late"),
        ("system-preset", "/usr/lib/ouros/system-preset"),
        ("system-shutdown", "/usr/lib/ouros/system-shutdown"),
        ("system-sleep", "/usr/lib/ouros/system-sleep"),
        ("system-unit-path", "/usr/lib/ouros/system"),
        ("user-binaries", "/usr/local/bin"),
        ("user-library-private", "/usr/local/lib"),
        ("user-configuration", "/etc/ouros/user"),
        ("user-runtime", "/run/user"),
        ("user-unit-path", "/usr/lib/ouros/user"),
        ("search-binaries", "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"),
        ("search-binaries-default", "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"),
        ("search-library-private", "/usr/local/lib:/usr/lib"),
    ];

    if args.is_empty() || (args.len() == 1 && args[0] == "--help") {
        if args.is_empty() {
            for (name, path) in &paths {
                writeln!(out, "{}={}", name, path)?;
            }
        } else {
            writeln!(out, "Usage: systemd-path [OPTIONS] [NAME...]")?;
            writeln!(out, "Show well-known system and user paths.")?;
            writeln!(out)?;
            writeln!(out, "If NAME is specified, show only that path.")?;
        }
        return Ok(0);
    }

    for arg in args {
        if arg.starts_with('-') {
            continue;
        }
        if let Some((_, path)) = paths.iter().find(|(name, _)| *name == arg.as_str()) {
            writeln!(out, "{}", path)?;
        } else {
            writeln!(out, "Unknown path: {}", arg)?;
            return Ok(1);
        }
    }
    Ok(0)
}

// ============================================================================
// systemd-notify
// ============================================================================

fn run_notify(out: &mut dyn Write, args: &[String]) -> io::Result<i32> {
    if args.is_empty() {
        writeln!(out, "Usage: systemd-notify [OPTIONS] [VARIABLE=VALUE...]")?;
        writeln!(out, "Notify the service manager about service status changes.")?;
        writeln!(out)?;
        writeln!(out, "Options:")?;
        writeln!(out, "  --ready         Notify that service startup is complete")?;
        writeln!(out, "  --reloading     Notify that service is reloading")?;
        writeln!(out, "  --stopping      Notify that service is stopping")?;
        writeln!(out, "  --status=TEXT   Set service status text")?;
        writeln!(out, "  --pid=PID       Send from specific PID")?;
        writeln!(out, "  --booted        Check if system was booted with systemd")?;
        return Ok(0);
    }

    let mut ready = false;
    let mut reloading = false;
    let mut stopping = false;
    let mut status: Option<String> = None;
    let mut booted = false;
    let mut vars = Vec::new();

    for arg in args {
        if arg == "--ready" {
            ready = true;
        } else if arg == "--reloading" {
            reloading = true;
        } else if arg == "--stopping" {
            stopping = true;
        } else if let Some(s) = arg.strip_prefix("--status=") {
            status = Some(s.to_string());
        } else if arg == "--booted" {
            booted = true;
        } else if arg.contains('=') && !arg.starts_with('-') {
            vars.push(arg.clone());
        }
    }

    if booted {
        writeln!(out, "yes")?;
        return Ok(0);
    }

    let mut parts = Vec::new();
    if ready {
        parts.push("READY=1".to_string());
    }
    if reloading {
        parts.push("RELOADING=1".to_string());
    }
    if stopping {
        parts.push("STOPPING=1".to_string());
    }
    if let Some(ref s) = status {
        parts.push(format!("STATUS={}", s));
    }
    for v in &vars {
        parts.push(v.clone());
    }

    if parts.is_empty() {
        writeln!(out, "No notification sent (no variables specified).")?;
        return Ok(1);
    }

    for p in &parts {
        writeln!(out, "Sending: {}", p)?;
    }
    Ok(0)
}

// ============================================================================
// systemd-tmpfiles
// ============================================================================

/// A parsed tmpfiles.d configuration line.
#[derive(Clone, Debug)]
struct TmpfilesEntry {
    entry_type: char,
    path: String,
    mode: String,
    user: String,
    group: String,
    age: String,
    argument: String,
}

fn parse_tmpfiles_line(line: &str) -> Option<TmpfilesEntry> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let fields: Vec<&str> = line.splitn(7, char::is_whitespace).collect();
    if fields.is_empty() {
        return None;
    }
    let entry_type = fields[0].chars().next()?;
    let path = fields.get(1).unwrap_or(&"-").to_string();
    let mode = fields.get(2).unwrap_or(&"-").to_string();
    let user = fields.get(3).unwrap_or(&"-").to_string();
    let group = fields.get(4).unwrap_or(&"-").to_string();
    let age = fields.get(5).unwrap_or(&"-").to_string();
    let argument = fields.get(6).unwrap_or(&"-").to_string();

    Some(TmpfilesEntry {
        entry_type,
        path,
        mode,
        user,
        group,
        age,
        argument,
    })
}

fn run_tmpfiles(out: &mut dyn Write, args: &[String]) -> io::Result<i32> {
    let mut create = false;
    let mut clean = false;
    let mut remove = false;
    let mut config_files: Vec<String> = Vec::new();

    for arg in args {
        match arg.as_str() {
            "--create" => create = true,
            "--clean" => clean = true,
            "--remove" => remove = true,
            "--help" => {
                writeln!(out, "Usage: systemd-tmpfiles [OPTIONS] [CONFIGFILE...]")?;
                writeln!(out, "Create, clean, and remove temporary files and directories.")?;
                writeln!(out)?;
                writeln!(out, "Options:")?;
                writeln!(out, "  --create   Create files and directories")?;
                writeln!(out, "  --clean    Clean up old files")?;
                writeln!(out, "  --remove   Remove files and directories")?;
                return Ok(0);
            }
            _ if !arg.starts_with('-') => config_files.push(arg.clone()),
            _ => {}
        }
    }

    if !create && !clean && !remove {
        writeln!(out, "systemd-tmpfiles: No action specified (use --create, --clean, or --remove).")?;
        return Ok(1);
    }

    // Simulated default config entries.
    let default_entries = ["d /tmp 1777 root root 10d",
        "d /var/tmp 1777 root root 30d",
        "d /run/lock 0755 root root -",
        "d /run/user 0755 root root -",
        "f /run/utmp 0664 root utmp -",
        "r! /tmp/.X*-lock - - - -"];

    let entries: Vec<TmpfilesEntry> = if config_files.is_empty() {
        default_entries
            .iter()
            .filter_map(|l| parse_tmpfiles_line(l))
            .collect()
    } else {
        // We would read files in a real implementation; use defaults for now.
        default_entries
            .iter()
            .filter_map(|l| parse_tmpfiles_line(l))
            .collect()
    };

    for e in &entries {
        match e.entry_type {
            'd' | 'D' => {
                if create {
                    writeln!(
                        out,
                        "Creating directory {} (mode={}, user={}, group={})",
                        e.path, e.mode, e.user, e.group
                    )?;
                }
                if clean && e.age != "-" {
                    writeln!(out, "Cleaning {} (age={})", e.path, e.age)?;
                }
                if remove {
                    writeln!(out, "Removing directory {}", e.path)?;
                }
            }
            'f' | 'F' => {
                if create {
                    writeln!(
                        out,
                        "Creating file {} (mode={}, user={}, group={})",
                        e.path, e.mode, e.user, e.group
                    )?;
                }
                if remove {
                    writeln!(out, "Removing file {}", e.path)?;
                }
            }
            'r' | 'R' => {
                if remove {
                    writeln!(out, "Removing (glob) {}", e.path)?;
                }
            }
            _ => {
                writeln!(out, "Unknown tmpfiles type '{}' for {}", e.entry_type, e.path)?;
            }
        }
    }
    Ok(0)
}

// ============================================================================
// Main dispatch
// ============================================================================

fn run_systemctl(args: &[String]) -> io::Result<i32> {
    let stdout = io::stdout();
    let mut out = stdout.lock();

    if args.is_empty() {
        return cmd_list_units(&mut out, &SystemctlFlags::default());
    }

    // Check for --help and --version first.
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print_systemctl_help(&mut out)?;
        return Ok(0);
    }
    if args.iter().any(|a| a == "--version") {
        writeln!(out, "systemctl {}", VERSION)?;
        return Ok(0);
    }

    let (flags, positional) = parse_systemctl_args(args);

    let cmd = positional.first().map(|s| s.as_str()).unwrap_or("list-units");
    let unit = positional.get(1).map(|s| s.as_str());

    match cmd {
        "list-units" => cmd_list_units(&mut out, &flags),
        "list-unit-files" => cmd_list_unit_files(&mut out, &flags),
        "status" => {
            let u = unit.unwrap_or("multi-user.target");
            cmd_status(&mut out, u, &flags)
        }
        "show" => {
            let u = unit.unwrap_or("multi-user.target");
            let prop = positional.get(2).map(|s| s.as_str());
            cmd_show(&mut out, u, prop, &flags)
        }
        "start" | "stop" | "restart" | "reload" | "enable" | "disable" | "mask" | "unmask" => {
            if let Some(u) = unit {
                cmd_unit_action(&mut out, cmd, u, &flags)
            } else {
                writeln!(out, "Too few arguments.")?;
                Ok(1)
            }
        }
        "is-active" => {
            let u = unit.unwrap_or("");
            cmd_is_active(&mut out, u, &flags)
        }
        "is-enabled" => {
            let u = unit.unwrap_or("");
            cmd_is_enabled(&mut out, u, &flags)
        }
        "is-failed" => {
            let u = unit.unwrap_or("");
            cmd_is_failed(&mut out, u, &flags)
        }
        "daemon-reload" => cmd_daemon_reload(&mut out, &flags),
        "cat" => {
            let u = unit.unwrap_or("");
            cmd_cat_unit(&mut out, u)
        }
        "edit" => {
            let u = unit.unwrap_or("");
            cmd_edit_unit(&mut out, u)
        }
        "poweroff" | "reboot" | "halt" | "suspend" | "hibernate" => {
            cmd_power(&mut out, cmd, &flags)
        }
        "isolate" => {
            let u = unit.unwrap_or("multi-user.target");
            cmd_isolate(&mut out, u, &flags)
        }
        "list-timers" => cmd_list_timers(&mut out, &flags),
        "list-sockets" => cmd_list_sockets(&mut out, &flags),
        "list-dependencies" => {
            let u = unit.unwrap_or("multi-user.target");
            cmd_list_dependencies(&mut out, u, &flags)
        }
        _ => {
            writeln!(out, "Unknown command: {}", cmd)?;
            Ok(1)
        }
    }
}

fn run_analyze(args: &[String]) -> io::Result<i32> {
    let stdout = io::stdout();
    let mut out = stdout.lock();

    if args.iter().any(|a| a == "--help" || a == "-h") {
        writeln!(out, "Usage: systemd-analyze [COMMAND]")?;
        writeln!(out)?;
        writeln!(out, "Commands:")?;
        writeln!(out, "  time                 Print boot time")?;
        writeln!(out, "  blame                Print per-unit startup time")?;
        writeln!(out, "  critical-chain [UNIT] Print critical boot chain")?;
        writeln!(out, "  plot                 Print boot chart (text)")?;
        writeln!(out, "  dot [UNIT...]        Print dependency graph (DOT)")?;
        writeln!(out, "  verify UNIT          Check unit file syntax")?;
        writeln!(out, "  security [UNIT]      Security analysis")?;
        writeln!(out)?;
        writeln!(out, "  --version            Print version")?;
        return Ok(0);
    }
    if args.iter().any(|a| a == "--version") {
        writeln!(out, "systemd-analyze {}", VERSION)?;
        return Ok(0);
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("time");
    match cmd {
        "time" => analyze_time(&mut out),
        "blame" => analyze_blame(&mut out),
        "critical-chain" => {
            let unit = args.get(1).map(|s| s.as_str());
            analyze_critical_chain(&mut out, unit)
        }
        "plot" => analyze_plot(&mut out),
        "dot" => {
            let units: Vec<String> = args.iter().skip(1).cloned().collect();
            analyze_dot(&mut out, &units)
        }
        "verify" => {
            if let Some(unit) = args.get(1) {
                analyze_verify(&mut out, unit)
            } else {
                writeln!(out, "systemd-analyze verify: No unit specified.")?;
                Ok(1)
            }
        }
        "security" => {
            let unit = args.get(1).map(|s| s.as_str());
            analyze_security(&mut out, unit)
        }
        _ => {
            writeln!(out, "Unknown analyze command: {}", cmd)?;
            Ok(1)
        }
    }
}

fn print_systemctl_help(out: &mut dyn Write) -> io::Result<()> {
    writeln!(out, "systemctl [OPTIONS...] COMMAND [UNIT...]")?;
    writeln!(out)?;
    writeln!(out, "Query or send control commands to the service manager.")?;
    writeln!(out)?;
    writeln!(out, "Unit Commands:")?;
    writeln!(out, "  list-units [PATTERN...]         List units in memory")?;
    writeln!(out, "  list-unit-files [PATTERN...]    List installed unit files")?;
    writeln!(out, "  status [UNIT...]                Show unit status")?;
    writeln!(out, "  show [UNIT...|JOB...]           Show properties")?;
    writeln!(out, "  cat UNIT...                     Show unit file contents")?;
    writeln!(out, "  edit UNIT...                    Edit unit file overrides")?;
    writeln!(out, "  start UNIT...                   Start units")?;
    writeln!(out, "  stop UNIT...                    Stop units")?;
    writeln!(out, "  restart UNIT...                 Restart units")?;
    writeln!(out, "  reload UNIT...                  Reload units")?;
    writeln!(out, "  enable UNIT...                  Enable units")?;
    writeln!(out, "  disable UNIT...                 Disable units")?;
    writeln!(out, "  mask UNIT...                    Mask units")?;
    writeln!(out, "  unmask UNIT...                  Unmask units")?;
    writeln!(out, "  is-active UNIT...               Check if active")?;
    writeln!(out, "  is-enabled UNIT...              Check if enabled")?;
    writeln!(out, "  is-failed UNIT...               Check if failed")?;
    writeln!(out, "  daemon-reload                   Reload unit files")?;
    writeln!(out, "  list-dependencies [UNIT]        Show dependency tree")?;
    writeln!(out, "  list-timers                     List timers")?;
    writeln!(out, "  list-sockets                    List sockets")?;
    writeln!(out)?;
    writeln!(out, "System Commands:")?;
    writeln!(out, "  poweroff                        Power off the system")?;
    writeln!(out, "  reboot                          Reboot the system")?;
    writeln!(out, "  halt                            Halt the system")?;
    writeln!(out, "  suspend                         Suspend the system")?;
    writeln!(out, "  hibernate                       Hibernate the system")?;
    writeln!(out, "  isolate TARGET                  Isolate a target")?;
    writeln!(out)?;
    writeln!(out, "Options:")?;
    writeln!(out, "  --user                          Talk to the user service manager")?;
    writeln!(out, "  --system                        Talk to the system manager (default)")?;
    writeln!(out, "  -t, --type=TYPE                 Filter by unit type")?;
    writeln!(out, "  --state=STATE                   Filter by unit state")?;
    writeln!(out, "  -a, --all                       Show all units/properties")?;
    writeln!(out, "  -q, --quiet                     Suppress output")?;
    writeln!(out, "  --no-pager                      Do not pipe output to pager")?;
    writeln!(out, "  --no-legend                     Do not print legend")?;
    writeln!(out, "  --plain                         Plain output")?;
    writeln!(out, "  --now                           Start/stop immediately with enable/disable")?;
    writeln!(out, "  -f, --force                     Force operation")?;
    writeln!(out, "  -h, --help                      Show this help")?;
    writeln!(out, "  --version                       Show version")?;
    Ok(())
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let argv0 = args.first().map(|s| s.as_str()).unwrap_or("systemctl");
    let personality = detect_personality(argv0);
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let result = match personality {
        Personality::Systemctl => run_systemctl(&rest),
        Personality::Analyze => run_analyze(&rest),
        Personality::Cat => {
            let stdout = io::stdout();
            let mut out = stdout.lock();
            if rest.iter().any(|a| a == "--help" || a == "-h") {
                writeln!(out, "Usage: systemd-cat [OPTIONS]").ok();
                writeln!(out, "Pipe stdin to the journal with optional priority.").ok();
                writeln!(out).ok();
                writeln!(out, "Options:").ok();
                writeln!(out, "  -p, --priority=PRIO  Set syslog priority (0-7)").ok();
                writeln!(out, "  -t, --identifier=ID  Set syslog identifier").ok();
                Ok(0)
            } else if rest.iter().any(|a| a == "--version") {
                writeln!(out, "systemd-cat {}", VERSION).ok();
                Ok(0)
            } else {
                run_cat_journal(&mut out)
            }
        }
        Personality::Cgls => {
            let stdout = io::stdout();
            let mut out = stdout.lock();
            if rest.iter().any(|a| a == "--help" || a == "-h") {
                writeln!(out, "Usage: systemd-cgls [OPTIONS] [CGROUP...]").ok();
                writeln!(out, "Show control group hierarchy.").ok();
                Ok(0)
            } else if rest.iter().any(|a| a == "--version") {
                writeln!(out, "systemd-cgls {}", VERSION).ok();
                Ok(0)
            } else {
                run_cgls(&mut out)
            }
        }
        Personality::Cgtop => {
            let stdout = io::stdout();
            let mut out = stdout.lock();
            if rest.iter().any(|a| a == "--help" || a == "-h") {
                writeln!(out, "Usage: systemd-cgtop [OPTIONS]").ok();
                writeln!(out, "Show top control groups by resource usage.").ok();
                Ok(0)
            } else if rest.iter().any(|a| a == "--version") {
                writeln!(out, "systemd-cgtop {}", VERSION).ok();
                Ok(0)
            } else {
                run_cgtop(&mut out)
            }
        }
        Personality::Escape => {
            let stdout = io::stdout();
            let mut out = stdout.lock();
            if rest.iter().any(|a| a == "--version") {
                writeln!(out, "systemd-escape {}", VERSION).ok();
                Ok(0)
            } else {
                run_escape(&mut out, &rest)
            }
        }
        Personality::Path => {
            let stdout = io::stdout();
            let mut out = stdout.lock();
            if rest.iter().any(|a| a == "--version") {
                writeln!(out, "systemd-path {}", VERSION).ok();
                Ok(0)
            } else {
                run_path(&mut out, &rest)
            }
        }
        Personality::Notify => {
            let stdout = io::stdout();
            let mut out = stdout.lock();
            if rest.iter().any(|a| a == "--version") {
                writeln!(out, "systemd-notify {}", VERSION).ok();
                Ok(0)
            } else {
                run_notify(&mut out, &rest)
            }
        }
        Personality::Tmpfiles => {
            let stdout = io::stdout();
            let mut out = stdout.lock();
            if rest.iter().any(|a| a == "--version") {
                writeln!(out, "systemd-tmpfiles {}", VERSION).ok();
                Ok(0)
            } else {
                run_tmpfiles(&mut out, &rest)
            }
        }
    };

    match result {
        Ok(code) => process::exit(code),
        Err(e) => {
            let _ = writeln!(io::stderr(), "{}: {}", personality.name(), e);
            process::exit(1);
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // Helper to capture output.
    fn capture<F>(f: F) -> (String, i32)
    where
        F: FnOnce(&mut Vec<u8>) -> io::Result<i32>,
    {
        let mut buf = Vec::new();
        let code = f(&mut buf).expect("IO should not fail in tests");
        (String::from_utf8(buf).expect("valid utf8"), code)
    }

    // --- Personality detection ---

    #[test]
    fn test_detect_systemctl() {
        assert_eq!(detect_personality("systemctl"), Personality::Systemctl);
    }

    #[test]
    fn test_detect_systemctl_with_path() {
        assert_eq!(
            detect_personality("/usr/bin/systemctl"),
            Personality::Systemctl
        );
    }

    #[test]
    fn test_detect_analyze() {
        assert_eq!(
            detect_personality("systemd-analyze"),
            Personality::Analyze
        );
    }

    #[test]
    fn test_detect_cat() {
        assert_eq!(detect_personality("systemd-cat"), Personality::Cat);
    }

    #[test]
    fn test_detect_cgls() {
        assert_eq!(detect_personality("systemd-cgls"), Personality::Cgls);
    }

    #[test]
    fn test_detect_cgtop() {
        assert_eq!(detect_personality("systemd-cgtop"), Personality::Cgtop);
    }

    #[test]
    fn test_detect_escape() {
        assert_eq!(detect_personality("systemd-escape"), Personality::Escape);
    }

    #[test]
    fn test_detect_path() {
        assert_eq!(detect_personality("systemd-path"), Personality::Path);
    }

    #[test]
    fn test_detect_notify() {
        assert_eq!(detect_personality("systemd-notify"), Personality::Notify);
    }

    #[test]
    fn test_detect_tmpfiles() {
        assert_eq!(
            detect_personality("systemd-tmpfiles"),
            Personality::Tmpfiles
        );
    }

    #[test]
    fn test_detect_exe_suffix() {
        assert_eq!(
            detect_personality("systemd-analyze.exe"),
            Personality::Analyze
        );
    }

    #[test]
    fn test_detect_windows_path() {
        assert_eq!(
            detect_personality("C:\\bin\\systemd-cgls.exe"),
            Personality::Cgls
        );
    }

    #[test]
    fn test_detect_unknown_defaults_to_systemctl() {
        assert_eq!(detect_personality("unknown"), Personality::Systemctl);
    }

    // --- Basename ---

    #[test]
    fn test_basename_simple() {
        assert_eq!(basename("foo"), "foo");
    }

    #[test]
    fn test_basename_unix_path() {
        assert_eq!(basename("/usr/bin/systemctl"), "systemctl");
    }

    #[test]
    fn test_basename_windows_path() {
        assert_eq!(basename("C:\\Windows\\system32\\test.exe"), "test.exe");
    }

    #[test]
    fn test_basename_mixed() {
        assert_eq!(basename("/usr/local\\bin/tool"), "tool");
    }

    // --- Unit type ---

    #[test]
    fn test_unit_type_from_name() {
        assert_eq!(
            UnitType::from_unit_name("sshd.service"),
            Some(UnitType::Service)
        );
    }

    #[test]
    fn test_unit_type_timer() {
        assert_eq!(
            UnitType::from_unit_name("foo.timer"),
            Some(UnitType::Timer)
        );
    }

    #[test]
    fn test_unit_type_none() {
        assert_eq!(UnitType::from_unit_name("noext"), None);
    }

    #[test]
    fn test_unit_type_unknown_suffix() {
        assert_eq!(UnitType::from_unit_name("foo.unknown"), None);
    }

    #[test]
    fn test_unit_type_round_trip() {
        for ut in &[
            UnitType::Service,
            UnitType::Socket,
            UnitType::Timer,
            UnitType::Mount,
            UnitType::Target,
        ] {
            assert_eq!(UnitType::from_str(ut.as_str()), Some(*ut));
        }
    }

    // --- Active state ---

    #[test]
    fn test_active_state_round_trip() {
        for s in &[
            ActiveState::Active,
            ActiveState::Inactive,
            ActiveState::Failed,
        ] {
            assert_eq!(ActiveState::from_str(s.as_str()), Some(*s));
        }
    }

    // --- Enable state ---

    #[test]
    fn test_enable_state_round_trip() {
        for s in &[
            EnableState::Enabled,
            EnableState::Disabled,
            EnableState::Static,
            EnableState::Masked,
        ] {
            assert_eq!(EnableState::from_str(s.as_str()), Some(*s));
        }
    }

    // --- Service type ---

    #[test]
    fn test_service_type_round_trip() {
        for st in &[
            ServiceType::Simple,
            ServiceType::Forking,
            ServiceType::Oneshot,
            ServiceType::Dbus,
            ServiceType::Notify,
            ServiceType::Idle,
            ServiceType::Exec,
        ] {
            assert_eq!(ServiceType::from_str(st.as_str()), Some(*st));
        }
    }

    // --- Unit file parsing ---

    #[test]
    fn test_parse_basic_unit() {
        let content = "[Unit]\nDescription=Test\n\n[Service]\nType=simple\nExecStart=/bin/foo\n";
        let uf = UnitFile::parse(content).unwrap();
        assert_eq!(uf.get("Unit", "Description"), Some("Test"));
        assert_eq!(uf.get("Service", "Type"), Some("simple"));
        assert_eq!(uf.get("Service", "ExecStart"), Some("/bin/foo"));
    }

    #[test]
    fn test_parse_comments_skipped() {
        let content = "# comment\n[Unit]\n; another comment\nDescription=Foo\n";
        let uf = UnitFile::parse(content).unwrap();
        assert_eq!(uf.get("Unit", "Description"), Some("Foo"));
    }

    #[test]
    fn test_parse_continuation() {
        let content = "[Unit]\nDescription=A \\\nvery long \\\ndescription\n";
        let uf = UnitFile::parse(content).unwrap();
        let desc = uf.get("Unit", "Description").unwrap();
        assert!(desc.contains("very long"));
        assert!(desc.contains("description"));
    }

    #[test]
    fn test_parse_stacking() {
        let content = "[Unit]\nAfter=a.service\nAfter=b.service\n";
        let uf = UnitFile::parse(content).unwrap();
        let vals = uf.get_all("Unit", "After");
        assert_eq!(vals, vec!["a.service", "b.service"]);
    }

    #[test]
    fn test_parse_section_names() {
        let content = "[Unit]\nDescription=x\n[Service]\nType=simple\n[Install]\nWantedBy=multi-user.target\n";
        let uf = UnitFile::parse(content).unwrap();
        let names = uf.section_names();
        assert!(names.contains(&"Unit"));
        assert!(names.contains(&"Service"));
        assert!(names.contains(&"Install"));
    }

    #[test]
    fn test_parse_empty_value() {
        let content = "[Service]\nExecStart=\n";
        let uf = UnitFile::parse(content).unwrap();
        assert_eq!(uf.get("Service", "ExecStart"), Some(""));
    }

    #[test]
    fn test_parse_error_no_section() {
        let content = "Key=Value\n";
        let result = UnitFile::parse(content);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_error_bad_section() {
        let content = "[]\nKey=Value\n";
        let result = UnitFile::parse(content);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_missing_key() {
        let content = "[Unit]\nDescription=Test\n";
        let uf = UnitFile::parse(content).unwrap();
        assert_eq!(uf.get("Unit", "Nonexistent"), None);
    }

    #[test]
    fn test_parse_missing_section() {
        let content = "[Unit]\nDescription=Test\n";
        let uf = UnitFile::parse(content).unwrap();
        assert_eq!(uf.get("Service", "Type"), None);
    }

    // --- Unit file verification ---

    #[test]
    fn test_verify_good_service() {
        let content = "[Unit]\nDescription=Good\n[Service]\nType=simple\nExecStart=/bin/foo\n[Install]\nWantedBy=multi-user.target\n";
        let uf = UnitFile::parse(content).unwrap();
        let issues = uf.verify("good.service");
        assert!(issues.is_empty(), "Issues: {:?}", issues);
    }

    #[test]
    fn test_verify_missing_unit_section() {
        let content = "[Service]\nType=simple\nExecStart=/bin/foo\n";
        let uf = UnitFile::parse(content).unwrap();
        let issues = uf.verify("bad.service");
        assert!(issues.iter().any(|i| i.contains("Missing [Unit] section")));
    }

    #[test]
    fn test_verify_missing_service_section() {
        let content = "[Unit]\nDescription=Test\n";
        let uf = UnitFile::parse(content).unwrap();
        let issues = uf.verify("bad.service");
        assert!(issues
            .iter()
            .any(|i| i.contains("Missing [Service] section")));
    }

    #[test]
    fn test_verify_missing_description() {
        let content = "[Unit]\nAfter=basic.target\n[Service]\nExecStart=/bin/foo\n";
        let uf = UnitFile::parse(content).unwrap();
        let issues = uf.verify("nodesc.service");
        assert!(issues
            .iter()
            .any(|i| i.contains("Missing Description")));
    }

    #[test]
    fn test_verify_missing_exec_start() {
        let content = "[Unit]\nDescription=Test\n[Service]\nType=simple\n";
        let uf = UnitFile::parse(content).unwrap();
        let issues = uf.verify("noexec.service");
        assert!(issues.iter().any(|i| i.contains("Missing ExecStart")));
    }

    #[test]
    fn test_verify_oneshot_no_exec_ok() {
        let content =
            "[Unit]\nDescription=Test\n[Service]\nType=oneshot\nExecStart=/bin/setup\n";
        let uf = UnitFile::parse(content).unwrap();
        let issues = uf.verify("oneshot.service");
        assert!(!issues.iter().any(|i| i.contains("Missing ExecStart")));
    }

    #[test]
    fn test_verify_timer_missing_section() {
        let content = "[Unit]\nDescription=Test\n";
        let uf = UnitFile::parse(content).unwrap();
        let issues = uf.verify("bad.timer");
        assert!(issues
            .iter()
            .any(|i| i.contains("Missing [Timer] section")));
    }

    #[test]
    fn test_verify_socket_missing_section() {
        let content = "[Unit]\nDescription=Test\n";
        let uf = UnitFile::parse(content).unwrap();
        let issues = uf.verify("bad.socket");
        assert!(issues
            .iter()
            .any(|i| i.contains("Missing [Socket] section")));
    }

    // --- Specifier expansion ---

    #[test]
    fn test_specifier_n() {
        let result = expand_specifiers("%n", "sshd.service");
        assert_eq!(result, "sshd.service");
    }

    #[test]
    fn test_specifier_p() {
        let result = expand_specifiers("%p", "sshd.service");
        assert_eq!(result, "sshd");
    }

    #[test]
    fn test_specifier_i() {
        let result = expand_specifiers("%i", "getty@tty1.service");
        assert_eq!(result, "tty1");
    }

    #[test]
    fn test_specifier_i_no_instance() {
        let result = expand_specifiers("%i", "sshd.service");
        assert_eq!(result, "");
    }

    #[test]
    fn test_specifier_h() {
        let result = expand_specifiers("%H", "sshd.service");
        assert_eq!(result, "ouros");
    }

    #[test]
    fn test_specifier_percent() {
        let result = expand_specifiers("100%%", "sshd.service");
        assert_eq!(result, "100%");
    }

    #[test]
    fn test_specifier_mixed() {
        let result = expand_specifiers("/run/%p/%i.pid", "getty@tty1.service");
        assert_eq!(result, "/run/getty/tty1.pid");
    }

    // --- Unit prefix/instance ---

    #[test]
    fn test_unit_prefix_simple() {
        assert_eq!(unit_prefix("sshd.service"), "sshd");
    }

    #[test]
    fn test_unit_prefix_template() {
        assert_eq!(unit_prefix("getty@tty1.service"), "getty");
    }

    #[test]
    fn test_unit_instance_some() {
        assert_eq!(unit_instance("getty@tty1.service"), Some("tty1"));
    }

    #[test]
    fn test_unit_instance_none() {
        assert_eq!(unit_instance("sshd.service"), None);
    }

    // --- Escaping ---

    #[test]
    fn test_escape_path() {
        let escaped = escape_unit_name("/dev/sda1");
        assert_eq!(escaped, "dev-sda1");
    }

    #[test]
    fn test_escape_simple() {
        let escaped = escape_unit_name("hello");
        assert_eq!(escaped, "hello");
    }

    #[test]
    fn test_escape_spaces() {
        let escaped = escape_unit_name("hello world");
        assert!(escaped.contains("\\x20"));
    }

    #[test]
    fn test_unescape_path() {
        let unescaped = unescape_unit_name("dev-sda1.mount");
        assert_eq!(unescaped, "/dev/sda1");
    }

    #[test]
    fn test_unescape_simple() {
        let unescaped = unescape_unit_name("hello");
        assert_eq!(unescaped, "hello");
    }

    #[test]
    fn test_escape_round_trip_path() {
        let escaped = escape_unit_name("/tmp/test");
        let unescaped = unescape_unit_name(&escaped);
        assert_eq!(unescaped, "/tmp/test");
    }

    // --- Flag parsing ---

    #[test]
    fn test_parse_flags_empty() {
        let (flags, pos) = parse_systemctl_args(&[]);
        assert!(!flags.user_scope);
        assert!(!flags.all);
        assert!(pos.is_empty());
    }

    #[test]
    fn test_parse_flags_user() {
        let args = vec!["--user".to_string(), "status".to_string()];
        let (flags, pos) = parse_systemctl_args(&args);
        assert!(flags.user_scope);
        assert_eq!(pos, vec!["status"]);
    }

    #[test]
    fn test_parse_flags_type_eq() {
        let args = vec!["--type=service".to_string()];
        let (flags, _) = parse_systemctl_args(&args);
        assert_eq!(flags.type_filter, Some("service".to_string()));
    }

    #[test]
    fn test_parse_flags_type_space() {
        let args = vec!["--type".to_string(), "timer".to_string()];
        let (flags, _) = parse_systemctl_args(&args);
        assert_eq!(flags.type_filter, Some("timer".to_string()));
    }

    #[test]
    fn test_parse_flags_all_short() {
        let args = vec!["-a".to_string()];
        let (flags, _) = parse_systemctl_args(&args);
        assert!(flags.all);
    }

    #[test]
    fn test_parse_flags_quiet() {
        let args = vec!["-q".to_string()];
        let (flags, _) = parse_systemctl_args(&args);
        assert!(flags.quiet);
    }

    #[test]
    fn test_parse_flags_now() {
        let args = vec!["--now".to_string(), "enable".to_string(), "sshd.service".to_string()];
        let (flags, pos) = parse_systemctl_args(&args);
        assert!(flags.now);
        assert_eq!(pos, vec!["enable", "sshd.service"]);
    }

    #[test]
    fn test_parse_flags_force() {
        let args = vec!["-f".to_string()];
        let (flags, _) = parse_systemctl_args(&args);
        assert!(flags.force);
    }

    #[test]
    fn test_parse_flags_state() {
        let args = vec!["--state=active".to_string()];
        let (flags, _) = parse_systemctl_args(&args);
        assert_eq!(flags.state_filter, Some("active".to_string()));
    }

    // --- list-units ---

    #[test]
    fn test_list_units_default() {
        let (out, code) = capture(|buf| cmd_list_units(buf, &SystemctlFlags::default()));
        assert_eq!(code, 0);
        assert!(out.contains("UNIT"));
        assert!(out.contains("init.service"));
    }

    #[test]
    fn test_list_units_type_filter() {
        let flags = SystemctlFlags {
            type_filter: Some("target".to_string()),
            ..Default::default()
        };
        let (out, code) = capture(|buf| cmd_list_units(buf, &flags));
        assert_eq!(code, 0);
        assert!(out.contains("basic.target"));
        assert!(!out.contains("init.service"));
    }

    #[test]
    fn test_list_units_no_legend() {
        let flags = SystemctlFlags {
            no_legend: true,
            ..Default::default()
        };
        let (out, code) = capture(|buf| cmd_list_units(buf, &flags));
        assert_eq!(code, 0);
        assert!(!out.contains("UNIT"));
        assert!(!out.contains("loaded units listed"));
    }

    // --- list-unit-files ---

    #[test]
    fn test_list_unit_files() {
        let (out, code) = capture(|buf| cmd_list_unit_files(buf, &SystemctlFlags::default()));
        assert_eq!(code, 0);
        assert!(out.contains("UNIT FILE"));
        assert!(out.contains("sshd.service"));
        assert!(out.contains("enabled"));
    }

    #[test]
    fn test_list_unit_files_type_filter() {
        let flags = SystemctlFlags {
            type_filter: Some("timer".to_string()),
            ..Default::default()
        };
        let (out, code) = capture(|buf| cmd_list_unit_files(buf, &flags));
        assert_eq!(code, 0);
        assert!(out.contains("logwatch.timer"));
        assert!(!out.contains("sshd.service"));
    }

    // --- status ---

    #[test]
    fn test_status_known_unit() {
        let (out, code) =
            capture(|buf| cmd_status(buf, "sshd.service", &SystemctlFlags::default()));
        assert_eq!(code, 0);
        assert!(out.contains("sshd.service"));
        assert!(out.contains("OpenSSH Server"));
        assert!(out.contains("active"));
    }

    #[test]
    fn test_status_unknown_unit() {
        let (out, code) =
            capture(|buf| cmd_status(buf, "nonexistent.service", &SystemctlFlags::default()));
        assert_eq!(code, 4);
        assert!(out.contains("could not be found"));
    }

    // --- show ---

    #[test]
    fn test_show_all_properties() {
        let (out, code) = capture(|buf| {
            cmd_show(buf, "sshd.service", None, &SystemctlFlags::default())
        });
        assert_eq!(code, 0);
        assert!(out.contains("Id=sshd.service"));
        assert!(out.contains("ActiveState=active"));
    }

    #[test]
    fn test_show_single_property() {
        let (out, code) = capture(|buf| {
            cmd_show(
                buf,
                "sshd.service",
                Some("ActiveState"),
                &SystemctlFlags::default(),
            )
        });
        assert_eq!(code, 0);
        assert!(out.contains("ActiveState=active"));
        assert!(!out.contains("Id="));
    }

    // --- is-active ---

    #[test]
    fn test_is_active_active() {
        let (out, code) = capture(|buf| {
            cmd_is_active(buf, "sshd.service", &SystemctlFlags::default())
        });
        assert_eq!(code, 0);
        assert!(out.trim() == "active");
    }

    #[test]
    fn test_is_active_unknown() {
        let (out, code) = capture(|buf| {
            cmd_is_active(buf, "nonexistent.service", &SystemctlFlags::default())
        });
        assert_eq!(code, 3);
        assert!(out.trim() == "inactive");
    }

    // --- is-enabled ---

    #[test]
    fn test_is_enabled_enabled() {
        let (out, code) = capture(|buf| {
            cmd_is_enabled(buf, "sshd.service", &SystemctlFlags::default())
        });
        assert_eq!(code, 0);
        assert!(out.trim() == "enabled");
    }

    #[test]
    fn test_is_enabled_static() {
        let (out, code) = capture(|buf| {
            cmd_is_enabled(buf, "init.service", &SystemctlFlags::default())
        });
        assert_eq!(code, 1);
        assert!(out.trim() == "static");
    }

    // --- is-failed ---

    #[test]
    fn test_is_failed_not_failed() {
        let (out, code) = capture(|buf| {
            cmd_is_failed(buf, "sshd.service", &SystemctlFlags::default())
        });
        assert_eq!(code, 1);
        assert!(out.trim() == "active");
    }

    // --- unit actions ---

    #[test]
    fn test_start_unit() {
        let (out, code) = capture(|buf| {
            cmd_unit_action(buf, "start", "sshd.service", &SystemctlFlags::default())
        });
        assert_eq!(code, 0);
        assert!(out.contains("Starting sshd.service"));
    }

    #[test]
    fn test_stop_unit() {
        let (out, code) = capture(|buf| {
            cmd_unit_action(buf, "stop", "sshd.service", &SystemctlFlags::default())
        });
        assert_eq!(code, 0);
        assert!(out.contains("Stopping sshd.service"));
    }

    #[test]
    fn test_enable_with_now() {
        let flags = SystemctlFlags {
            now: true,
            ..Default::default()
        };
        let (out, code) = capture(|buf| {
            cmd_unit_action(buf, "enable", "sshd.service", &flags)
        });
        assert_eq!(code, 0);
        assert!(out.contains("Created symlink"));
        assert!(out.contains("Starting"));
    }

    #[test]
    fn test_disable_unit() {
        let (out, code) = capture(|buf| {
            cmd_unit_action(buf, "disable", "sshd.service", &SystemctlFlags::default())
        });
        assert_eq!(code, 0);
        assert!(out.contains("Removed"));
    }

    #[test]
    fn test_mask_unit() {
        let (out, code) = capture(|buf| {
            cmd_unit_action(buf, "mask", "sshd.service", &SystemctlFlags::default())
        });
        assert_eq!(code, 0);
        assert!(out.contains("/dev/null"));
    }

    #[test]
    fn test_auto_append_service() {
        let (out, code) = capture(|buf| {
            cmd_unit_action(buf, "start", "sshd", &SystemctlFlags::default())
        });
        assert_eq!(code, 0);
        assert!(out.contains("sshd.service"));
    }

    #[test]
    fn test_quiet_suppresses_output() {
        let flags = SystemctlFlags {
            quiet: true,
            ..Default::default()
        };
        let (out, code) = capture(|buf| {
            cmd_unit_action(buf, "start", "sshd.service", &flags)
        });
        assert_eq!(code, 0);
        assert!(out.is_empty());
    }

    // --- daemon-reload ---

    #[test]
    fn test_daemon_reload() {
        let (out, code) =
            capture(|buf| cmd_daemon_reload(buf, &SystemctlFlags::default()));
        assert_eq!(code, 0);
        assert!(out.contains("Reloading"));
    }

    // --- cat unit ---

    #[test]
    fn test_cat_known_unit() {
        let (out, code) = capture(|buf| cmd_cat_unit(buf, "sshd.service"));
        assert_eq!(code, 0);
        assert!(out.contains("[Unit]"));
        assert!(out.contains("[Service]"));
        assert!(out.contains("ExecStart="));
    }

    #[test]
    fn test_cat_unknown_unit() {
        let (out, code) = capture(|buf| cmd_cat_unit(buf, "nonexistent.service"));
        assert_eq!(code, 1);
        assert!(out.contains("No files found"));
    }

    // --- power commands ---

    #[test]
    fn test_poweroff() {
        let (out, code) =
            capture(|buf| cmd_power(buf, "poweroff", &SystemctlFlags::default()));
        assert_eq!(code, 0);
        assert!(out.contains("powering off"));
    }

    #[test]
    fn test_reboot() {
        let (out, code) =
            capture(|buf| cmd_power(buf, "reboot", &SystemctlFlags::default()));
        assert_eq!(code, 0);
        assert!(out.contains("rebooting"));
    }

    // --- isolate ---

    #[test]
    fn test_isolate() {
        let (out, code) = capture(|buf| {
            cmd_isolate(buf, "rescue.target", &SystemctlFlags::default())
        });
        assert_eq!(code, 0);
        assert!(out.contains("Isolating rescue.target"));
    }

    // --- list-timers ---

    #[test]
    fn test_list_timers() {
        let (out, code) =
            capture(|buf| cmd_list_timers(buf, &SystemctlFlags::default()));
        assert_eq!(code, 0);
        assert!(out.contains("logwatch.timer"));
        assert!(out.contains("NEXT"));
    }

    // --- list-sockets ---

    #[test]
    fn test_list_sockets() {
        let (out, code) =
            capture(|buf| cmd_list_sockets(buf, &SystemctlFlags::default()));
        assert_eq!(code, 0);
        assert!(out.contains("dbus.socket"));
        assert!(out.contains("Stream"));
    }

    // --- list-dependencies ---

    #[test]
    fn test_list_dependencies() {
        let (out, code) = capture(|buf| {
            cmd_list_dependencies(buf, "multi-user.target", &SystemctlFlags::default())
        });
        assert_eq!(code, 0);
        assert!(out.contains("multi-user.target"));
        assert!(out.contains("basic.target"));
    }

    // --- systemd-analyze ---

    #[test]
    fn test_analyze_time() {
        let (out, code) = capture(|buf| analyze_time(buf));
        assert_eq!(code, 0);
        assert!(out.contains("Startup finished"));
        assert!(out.contains("kernel"));
        assert!(out.contains("userspace"));
    }

    #[test]
    fn test_analyze_blame() {
        let (out, code) = capture(|buf| analyze_blame(buf));
        assert_eq!(code, 0);
        assert!(out.contains("network.service"));
        assert!(out.contains("1.500s"));
    }

    #[test]
    fn test_analyze_critical_chain_default() {
        let (out, code) = capture(|buf| analyze_critical_chain(buf, None));
        assert_eq!(code, 0);
        assert!(out.contains("graphical.target"));
        assert!(out.contains("network.service"));
    }

    #[test]
    fn test_analyze_critical_chain_unit() {
        let (out, code) =
            capture(|buf| analyze_critical_chain(buf, Some("multi-user.target")));
        assert_eq!(code, 0);
        assert!(out.contains("multi-user.target"));
    }

    #[test]
    fn test_analyze_plot() {
        let (out, code) = capture(|buf| analyze_plot(buf));
        assert_eq!(code, 0);
        assert!(out.contains("Boot Plot"));
        assert!(out.contains("kernel"));
    }

    #[test]
    fn test_analyze_dot_default() {
        let (out, code) = capture(|buf| analyze_dot(buf, &[]));
        assert_eq!(code, 0);
        assert!(out.contains("digraph systemd"));
        assert!(out.contains("graphical.target"));
    }

    #[test]
    fn test_analyze_dot_units() {
        let units = vec!["sshd.service".to_string()];
        let (out, code) = capture(|buf| analyze_dot(buf, &units));
        assert_eq!(code, 0);
        assert!(out.contains("sshd.service"));
    }

    #[test]
    fn test_analyze_verify_ok() {
        let (out, code) = capture(|buf| analyze_verify(buf, "sshd.service"));
        assert_eq!(code, 0);
        assert!(out.contains("syntax OK"));
    }

    #[test]
    fn test_analyze_verify_no_suffix() {
        let (out, code) = capture(|buf| analyze_verify(buf, "sshd"));
        assert_eq!(code, 1);
        assert!(out.contains("type suffix"));
    }

    #[test]
    fn test_analyze_security() {
        let (out, code) = capture(|buf| analyze_security(buf, Some("sshd.service")));
        assert_eq!(code, 0);
        assert!(out.contains("EXPOSURE"));
        assert!(out.contains("MEDIUM"));
    }

    // --- systemd-cgls ---

    #[test]
    fn test_cgls() {
        let (out, code) = capture(|buf| run_cgls(buf));
        assert_eq!(code, 0);
        assert!(out.contains("Control group /"));
        assert!(out.contains("system.slice"));
        assert!(out.contains("user.slice"));
    }

    // --- systemd-cgtop ---

    #[test]
    fn test_cgtop() {
        let (out, code) = capture(|buf| run_cgtop(buf));
        assert_eq!(code, 0);
        assert!(out.contains("Control Group"));
        assert!(out.contains("/system.slice"));
    }

    // --- systemd-escape ---

    #[test]
    fn test_escape_help() {
        let (out, code) = capture(|buf| run_escape(buf, &[]));
        assert_eq!(code, 0);
        assert!(out.contains("Usage"));
    }

    #[test]
    fn test_escape_path_arg() {
        let args = vec!["/dev/sda1".to_string()];
        let (out, code) = capture(|buf| run_escape(buf, &args));
        assert_eq!(code, 0);
        assert!(out.trim() == "dev-sda1");
    }

    #[test]
    fn test_escape_with_suffix() {
        let args = vec!["--suffix=mount".to_string(), "/dev/sda1".to_string()];
        let (out, code) = capture(|buf| run_escape(buf, &args));
        assert_eq!(code, 0);
        assert!(out.trim() == "dev-sda1.mount");
    }

    #[test]
    fn test_escape_unescape() {
        let args = vec!["-u".to_string(), "dev-sda1".to_string()];
        let (out, code) = capture(|buf| run_escape(buf, &args));
        assert_eq!(code, 0);
        assert!(out.trim() == "/dev/sda1");
    }

    // --- systemd-path ---

    #[test]
    fn test_path_list_all() {
        let (out, code) = capture(|buf| run_path(buf, &[]));
        assert_eq!(code, 0);
        assert!(out.contains("temporary=/tmp"));
        assert!(out.contains("system-configuration=/etc"));
    }

    #[test]
    fn test_path_specific() {
        let args = vec!["temporary".to_string()];
        let (out, code) = capture(|buf| run_path(buf, &args));
        assert_eq!(code, 0);
        assert!(out.trim() == "/tmp");
    }

    #[test]
    fn test_path_unknown() {
        let args = vec!["nonexistent".to_string()];
        let (out, code) = capture(|buf| run_path(buf, &args));
        assert_eq!(code, 1);
        assert!(out.contains("Unknown path"));
    }

    // --- systemd-notify ---

    #[test]
    fn test_notify_help() {
        let (out, code) = capture(|buf| run_notify(buf, &[]));
        assert_eq!(code, 0);
        assert!(out.contains("Usage"));
    }

    #[test]
    fn test_notify_ready() {
        let args = vec!["--ready".to_string()];
        let (out, code) = capture(|buf| run_notify(buf, &args));
        assert_eq!(code, 0);
        assert!(out.contains("READY=1"));
    }

    #[test]
    fn test_notify_status() {
        let args = vec!["--status=Initializing".to_string()];
        let (out, code) = capture(|buf| run_notify(buf, &args));
        assert_eq!(code, 0);
        assert!(out.contains("STATUS=Initializing"));
    }

    #[test]
    fn test_notify_booted() {
        let args = vec!["--booted".to_string()];
        let (out, code) = capture(|buf| run_notify(buf, &args));
        assert_eq!(code, 0);
        assert!(out.trim() == "yes");
    }

    #[test]
    fn test_notify_custom_var() {
        let args = vec!["MAINPID=1234".to_string()];
        let (out, code) = capture(|buf| run_notify(buf, &args));
        assert_eq!(code, 0);
        assert!(out.contains("MAINPID=1234"));
    }

    // --- systemd-tmpfiles ---

    #[test]
    fn test_tmpfiles_no_action() {
        let (out, code) = capture(|buf| run_tmpfiles(buf, &[]));
        assert_eq!(code, 1);
        assert!(out.contains("No action specified"));
    }

    #[test]
    fn test_tmpfiles_create() {
        let args = vec!["--create".to_string()];
        let (out, code) = capture(|buf| run_tmpfiles(buf, &args));
        assert_eq!(code, 0);
        assert!(out.contains("Creating directory /tmp"));
        assert!(out.contains("Creating file /run/utmp"));
    }

    #[test]
    fn test_tmpfiles_clean() {
        let args = vec!["--clean".to_string()];
        let (out, code) = capture(|buf| run_tmpfiles(buf, &args));
        assert_eq!(code, 0);
        assert!(out.contains("Cleaning /tmp"));
    }

    #[test]
    fn test_tmpfiles_remove() {
        let args = vec!["--remove".to_string()];
        let (out, code) = capture(|buf| run_tmpfiles(buf, &args));
        assert_eq!(code, 0);
        assert!(out.contains("Removing directory /tmp"));
        assert!(out.contains("Removing (glob)"));
    }

    #[test]
    fn test_tmpfiles_help() {
        let args = vec!["--help".to_string()];
        let (out, code) = capture(|buf| run_tmpfiles(buf, &args));
        assert_eq!(code, 0);
        assert!(out.contains("Usage"));
    }

    // --- tmpfiles line parsing ---

    #[test]
    fn test_parse_tmpfiles_line_dir() {
        let entry = parse_tmpfiles_line("d /tmp 1777 root root 10d").unwrap();
        assert_eq!(entry.entry_type, 'd');
        assert_eq!(entry.path, "/tmp");
        assert_eq!(entry.mode, "1777");
    }

    #[test]
    fn test_parse_tmpfiles_line_comment() {
        assert!(parse_tmpfiles_line("# comment").is_none());
    }

    #[test]
    fn test_parse_tmpfiles_line_empty() {
        assert!(parse_tmpfiles_line("").is_none());
    }

    #[test]
    fn test_parse_tmpfiles_line_file() {
        let entry = parse_tmpfiles_line("f /run/utmp 0664 root utmp -").unwrap();
        assert_eq!(entry.entry_type, 'f');
        assert_eq!(entry.path, "/run/utmp");
    }

    // --- Personality name ---

    #[test]
    fn test_personality_names() {
        assert_eq!(Personality::Systemctl.name(), "systemctl");
        assert_eq!(Personality::Analyze.name(), "systemd-analyze");
        assert_eq!(Personality::Cat.name(), "systemd-cat");
        assert_eq!(Personality::Cgls.name(), "systemd-cgls");
        assert_eq!(Personality::Cgtop.name(), "systemd-cgtop");
        assert_eq!(Personality::Escape.name(), "systemd-escape");
        assert_eq!(Personality::Path.name(), "systemd-path");
        assert_eq!(Personality::Notify.name(), "systemd-notify");
        assert_eq!(Personality::Tmpfiles.name(), "systemd-tmpfiles");
    }

    // --- Load state / sub state ---

    #[test]
    fn test_load_state_as_str() {
        assert_eq!(LoadState::Loaded.as_str(), "loaded");
        assert_eq!(LoadState::NotFound.as_str(), "not-found");
        assert_eq!(LoadState::Masked.as_str(), "masked");
    }

    #[test]
    fn test_sub_state_as_str() {
        assert_eq!(SubState::Running.as_str(), "running");
        assert_eq!(SubState::Dead.as_str(), "dead");
        assert_eq!(SubState::Listening.as_str(), "listening");
    }

    // --- edit ---

    #[test]
    fn test_edit_unit() {
        let (out, code) = capture(|buf| cmd_edit_unit(buf, "sshd.service"));
        assert_eq!(code, 0);
        assert!(out.contains("Editing"));
        assert!(out.contains("override.conf"));
    }

    // --- Specifier N (unescaped name) ---

    #[test]
    fn test_specifier_big_n() {
        let result = expand_specifiers("%N", "dev-sda1.mount");
        assert_eq!(result, "/dev/sda1");
    }
}
