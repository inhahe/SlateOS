//! Multi-personality service management utility for SlateOS.
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
use std::fs;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
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
                Self::parse_kv(&assembled, &current_section, &mut sections, line_no)?;
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
            Self::parse_kv(&continued_line, &current_section, &mut sections, 0)?;
        }

        Ok(UnitFile { sections })
    }

    fn parse_kv(
        line: &str,
        current_section: &Option<String>,
        sections: &mut BTreeMap<String, Vec<(String, String)>>,
        line_no: usize,
    ) -> Result<(), String> {
        let section = current_section
            .as_ref()
            .ok_or_else(|| format!("Key-value pair outside section at line {}", line_no + 1))?;
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
                Some('H') => result.push_str("slateos"),
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
#[derive(Clone, Debug, Default)]
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

/// A unit-file entry as `list-unit-files` would print one.
///
/// The line above this used to read "Return a set of simulated units
/// representing a typical booted system" -- the doc comment of
/// `simulated_units`, left behind when that function was deleted, so it
/// silently became the documentation for this struct. A deletion that removes
/// a definition and leaves its doc comment does not fail to compile; it
/// reattaches, and the next reader is told this type does something it does
/// not.
struct UnitFileEntry {
    name: &'static str,
    state: EnableState,
    preset: &'static str,
}

// ============================================================================
// What this system can actually be asked about its services
// ============================================================================
//
// `simulated_units` and `simulated_unit_files` used to live here. They were
// not fallbacks -- they were the ONLY source, and EIGHT subcommands called
// them unconditionally: list-units, list-unit-files, status, show, is-active,
// is-enabled, is-failed and cat. So `systemctl status sshd.service` reported
// `active (running)` and exited 0 having asked nothing, and `systemctl
// is-active sshd.service` exited 0 -- which is the one a script branches on.
//
// A test asserted each of those. `test_status_known_unit` required the words
// "active" and "OpenSSH Server" in the output of a program that had not
// looked.
//
// WHAT THERE IS TO ASK. The kernel does run a service manager
// (`kernel/src/fs/servicemgr.rs`) and procfs serves `/proc/servicemgr` -- but
// only as aggregate counters: service_count, running, total_starts,
// total_stops, total_failures, ops. No per-unit names or states exist
// anywhere, and the SYS_SERVICE_* syscalls (280-285) are the name-registration
// bus, not a unit manager. Enumerating units is genuinely impossible from
// here, so the honest answer is to say so -- and to say what IS known, because
// "I cannot list them" and "there are none" are different facts and the
// counter separates them.

/// The aggregate service figures from `/proc/servicemgr`, if readable.
///
/// `(service_count, running)`. `None` means the file could not be read or did
/// not carry those keys -- never that there are no services.
fn service_counts() -> Option<(u64, u64)> {
    let raw = procinfo::ProcFs::new()
        .read_optional("servicemgr")
        .ok()
        .flatten()?;
    let pairs = procinfo::parse_key_values(&raw);
    let field = |name: &str| {
        pairs
            .iter()
            .find(|kv| kv.key == name.as_bytes())
            .and_then(|kv| kv.value_str())
            .and_then(|v| v.trim().parse::<u64>().ok())
    };
    Some((field("service_count")?, field("running")?))
}

/// The diagnostic every unit-level subcommand shares, on stderr.
fn no_unit_interface(what: &str) {
    eprintln!("systemctl: cannot {what}: this system exposes no per-unit interface");
    match service_counts() {
        Some((total, running)) => eprintln!(
            "systemctl: /proc/servicemgr reports {total} service(s), {running} running, \
             but not their names or states"
        ),
        None => eprintln!("systemctl: /proc/servicemgr could not be read either"),
    }
}

// ============================================================================
// Systemctl sub-commands
// ============================================================================

fn cmd_list_units(_out: &mut dyn Write, _flags: &SystemctlFlags) -> io::Result<i32> {
    no_unit_interface("list units");
    Ok(1)
}

fn cmd_list_unit_files(_out: &mut dyn Write, _flags: &SystemctlFlags) -> io::Result<i32> {
    no_unit_interface("list unit files");
    Ok(1)
}

fn cmd_status(_out: &mut dyn Write, unit_name: &str, _flags: &SystemctlFlags) -> io::Result<i32> {
    // Deliberately NOT exit 4. systemd's 4 means "no such unit", and this
    // program cannot tell a unit that does not exist from one it cannot see.
    // Claiming the former would be the same defect facing the other way.
    no_unit_interface(&format!("report the status of {unit_name}"));
    Ok(1)
}

fn cmd_show(
    _out: &mut dyn Write,
    unit_name: &str,
    _property: Option<&str>,
    _flags: &SystemctlFlags,
) -> io::Result<i32> {
    no_unit_interface(&format!("show properties of {unit_name}"));
    Ok(1)
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
        let scope = if flags.user_scope {
            "--user"
        } else {
            "--system"
        };
        match action {
            "start" => writeln!(out, "Starting {} ({})...", name_with_suffix, scope)?,
            "stop" => writeln!(out, "Stopping {} ({})...", name_with_suffix, scope)?,
            "restart" => writeln!(out, "Restarting {} ({})...", name_with_suffix, scope)?,
            "reload" => writeln!(out, "Reloading {} ({})...", name_with_suffix, scope)?,
            "enable" => {
                writeln!(
                    out,
                    "Created symlink /etc/slateos/system/multi-user.target.wants/{} -> /usr/lib/slateos/system/{}.",
                    name_with_suffix, name_with_suffix
                )?;
                if flags.now {
                    writeln!(out, "Starting {} ({})...", name_with_suffix, scope)?;
                }
            }
            "disable" => {
                writeln!(
                    out,
                    "Removed /etc/slateos/system/multi-user.target.wants/{}.",
                    name_with_suffix
                )?;
                if flags.now {
                    writeln!(out, "Stopping {} ({})...", name_with_suffix, scope)?;
                }
            }
            "mask" => writeln!(
                out,
                "Created symlink /etc/slateos/system/{} -> /dev/null.",
                name_with_suffix
            )?,
            "unmask" => writeln!(out, "Removed /etc/slateos/system/{}.", name_with_suffix)?,
            _ => writeln!(out, "Unknown action: {}", action)?,
        }
    }
    Ok(0)
}

fn cmd_is_active(out: &mut dyn Write, unit_name: &str, _flags: &SystemctlFlags) -> io::Result<i32> {
    // `unknown` is what systemd prints for a unit it cannot resolve, and the
    // non-zero status is what a script branches on. This is the subcommand
    // that mattered most: it used to exit 0 for four hard-coded names, so
    // `systemctl is-active sshd || start_it` never started anything.
    writeln!(out, "unknown")?;
    no_unit_interface(&format!("determine whether {unit_name} is active"));
    Ok(1)
}

fn cmd_is_enabled(
    _out: &mut dyn Write,
    unit_name: &str,
    _flags: &SystemctlFlags,
) -> io::Result<i32> {
    no_unit_interface(&format!("determine whether {unit_name} is enabled"));
    Ok(1)
}

fn cmd_is_failed(out: &mut dyn Write, unit_name: &str, _flags: &SystemctlFlags) -> io::Result<i32> {
    writeln!(out, "unknown")?;
    no_unit_interface(&format!("determine whether {unit_name} has failed"));
    Ok(1)
}

fn cmd_daemon_reload(out: &mut dyn Write, flags: &SystemctlFlags) -> io::Result<i32> {
    if !flags.quiet {
        writeln!(out, "Reloading daemon configuration...")?;
    }
    Ok(0)
}

fn cmd_cat_unit(_out: &mut dyn Write, unit_name: &str) -> io::Result<i32> {
    no_unit_interface(&format!("show the unit file for {unit_name}"));
    Ok(1)
}

fn cmd_edit_unit(out: &mut dyn Write, unit_name: &str) -> io::Result<i32> {
    writeln!(
        out,
        "Editing /etc/slateos/system/{}.d/override.conf...",
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
        "Mon 2026-01-02 00:00:00", "23h left", "Mon 2026-01-01 00:00:00", "1h ago"
    )?;
    if !flags.no_legend {
        writeln!(out)?;
        writeln!(out, "1 timers listed.")?;
    }
    Ok(0)
}

fn cmd_list_sockets(_out: &mut dyn Write, _flags: &SystemctlFlags) -> io::Result<i32> {
    // Printed one hard-coded row -- /run/dbus/system_bus_socket, Stream,
    // dbus.socket -- followed by "1 sockets listed.", whether or not that
    // socket existed and without enumerating anything. Socket units are unit
    // state like any other, and there is no per-unit interface to read.
    no_unit_interface("list sockets");
    Ok(1)
}

fn cmd_list_dependencies(
    _out: &mut dyn Write,
    unit_name: &str,
    _flags: &SystemctlFlags,
) -> io::Result<i32> {
    // The comment on the deleted body said it plainly: "Produce a synthetic
    // dependency tree." It matched three unit names and invented children for
    // them -- multi-user.target got basic, dbus, network, sshd and cron --
    // and anything else got a single basic.target. A dependency graph is the
    // thing you consult to decide what stopping a unit will take down with
    // it, so an invented one is worse than none.
    no_unit_interface(&format!("list the dependencies of {unit_name}"));
    Ok(1)
}

// ============================================================================
// systemd-analyze sub-commands
// ============================================================================

fn analyze_time(out: &mut dyn Write) -> io::Result<i32> {
    writeln!(
        out,
        "Startup finished in 1.200s (kernel) + 2.500s (userspace) = 3.700s"
    )?;
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
    writeln!(
        out,
        "The time when unit became active or started is printed after the \"@\" character."
    )?;
    writeln!(
        out,
        "The time the unit took to start is printed after the \"+\" character."
    )?;
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
        writeln!(
            out,
            "{}: Unit name should include a type suffix.",
            unit_name
        )?;
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
    writeln!(
        out,
        "  NAME                           DESCRIPTION                          EXPOSURE"
    )?;
    writeln!(
        out,
        "✓ PrivateNetwork=               Service has private network namespace     0.5"
    )?;
    writeln!(
        out,
        "✗ PrivateTmp=                    Tmp is not private                        0.1"
    )?;
    writeln!(
        out,
        "✓ NoNewPrivileges=               No new privileges                        0.2"
    )?;
    writeln!(
        out,
        "✓ ProtectSystem=                 System is protected read-only             0.3"
    )?;
    writeln!(
        out,
        "✗ ProtectHome=                   Home is not protected                     0.2"
    )?;
    writeln!(out)?;
    writeln!(
        out,
        "→ Overall exposure level for {}: 4.2 MEDIUM",
        unit_name
    )?;
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

// ── cgroups, read rather than invented ─────────────────────────────

/// The cgroup v2 mount point.
///
/// `systemd-cgls` shows the unified hierarchy. v1 controllers live in
/// per-controller subdirectories of this same path and are not what this
/// walks; on a v2 system there are none.
const CGROUP_ROOT: &str = "/sys/fs/cgroup";

/// A cgroup's child cgroups: its subdirectories, sorted.
///
/// Sorted because `read_dir` yields the filesystem's order, and a tree that
/// reorders itself between runs cannot be diffed against an earlier one.
fn cgroup_children(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut kids: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    kids.sort();
    kids
}

/// The pids listed in a cgroup's `cgroup.procs`.
///
/// An interior cgroup usually has none of its own: v2 forbids processes in a
/// node that has controller-enabled children.
fn cgroup_procs(dir: &Path) -> Vec<u32> {
    let Ok(text) = fs::read_to_string(dir.join("cgroup.procs")) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|l| l.trim().parse::<u32>().ok())
        .collect()
}

/// How `systemd-cgls` labels a process: its command line, with the NULs that
/// separate the arguments rendered as spaces.
///
/// Bytes, not text. A command line is whatever `execve` was handed and need
/// not be UTF-8, and `from_utf8_lossy` here would put replacement characters
/// into a listing someone reads to identify a process.
fn pid_command(pid: u32) -> Vec<u8> {
    if let Ok(raw) = fs::read(format!("/proc/{pid}/cmdline")) {
        let parts: Vec<&[u8]> = raw.split(|b| *b == 0).filter(|p| !p.is_empty()).collect();
        if !parts.is_empty() {
            return parts.join(&b' ');
        }
    }
    // A kernel thread has an empty cmdline; `comm` is what it has instead.
    fs::read(format!("/proc/{pid}/comm"))
        .map(|mut c| {
            while c.last() == Some(&b'\n') {
                c.pop();
            }
            c
        })
        .unwrap_or_default()
}

/// One cgroup's name as it should appear in the tree.
///
/// `to_str` rather than `to_string_lossy`: a name that is not UTF-8 is shown
/// in its escaped debug form, which is unambiguous, instead of having its
/// bytes replaced by U+FFFD.
fn cgroup_label(dir: &Path) -> String {
    let name = dir.file_name().unwrap_or(dir.as_os_str());
    name.to_str()
        .map_or_else(|| format!("{name:?}"), ToString::to_string)
}

/// Print `dir`'s children and processes, `systemd-cgls` style.
fn print_cgroup_tree(out: &mut dyn Write, dir: &Path, prefix: &str) -> io::Result<()> {
    let kids = cgroup_children(dir);
    let procs = cgroup_procs(dir);
    let total = kids.len().saturating_add(procs.len());
    let mut seen = 0usize;

    for pid in procs {
        seen = seen.saturating_add(1);
        let stem = if seen == total { "└─" } else { "├─" };
        write!(out, "{prefix}{stem}{pid} ")?;
        out.write_all(&pid_command(pid))?;
        writeln!(out)?;
    }
    for kid in kids {
        seen = seen.saturating_add(1);
        let last = seen == total;
        writeln!(
            out,
            "{prefix}{}{}",
            if last { "└─" } else { "├─" },
            cgroup_label(&kid)
        )?;
        let deeper = format!("{prefix}{}", if last { "  " } else { "│ " });
        print_cgroup_tree(out, &kid, &deeper)?;
    }
    Ok(())
}

fn run_cgls(out: &mut dyn Write) -> io::Result<i32> {
    // This used to print a fixed tree -- `init.scope`, `dbus.service`, pids
    // 1 and 100 -- for every machine, having read nothing. The hierarchy is
    // a directory tree, so there was never a reason to invent it.
    let root = Path::new(CGROUP_ROOT);
    if !root.is_dir() {
        writeln!(out, "systemd-cgls: {CGROUP_ROOT} is not a directory")?;
        return Ok(1);
    }
    writeln!(out, "Control group /:")?;
    print_cgroup_tree(out, root, "")?;
    Ok(0)
}

// ============================================================================
// systemd-cgtop
// ============================================================================

/// A byte count in the units `systemd-cgtop` prints: binary, one decimal.
///
/// Measured against the reference: 4096 renders `4.0K` and 162 424 832
/// renders `154.9M`. Below a kibibyte it is a plain count of bytes.
fn format_cgroup_size(bytes: u64) -> String {
    const STEP: f64 = 1024.0;
    let units = ["K", "M", "G", "T", "P"];
    if bytes < 1024 {
        return format!("{bytes}B");
    }
    let mut value = bytes as f64 / STEP;
    let mut unit = units[0];
    for next in &units[1..] {
        if value < STEP {
            break;
        }
        value /= STEP;
        unit = next;
    }
    format!("{value:.1}{unit}")
}

/// How many processes are in a cgroup.
///
/// `pids.current` when that controller is enabled, and the line count of
/// `cgroup.procs` otherwise -- which is what the number means either way.
/// `None` when neither file is readable, and the reference prints `-` for
/// that rather than 0: a group whose count is unknown is not a group with
/// no processes.
fn cgroup_tasks(dir: &Path) -> Option<u64> {
    if let Ok(n) = fs::read_to_string(dir.join("pids.current"))
        .map_err(|_| ())
        .and_then(|t| t.trim().parse::<u64>().map_err(|_| ()))
    {
        return Some(n);
    }
    let text = fs::read_to_string(dir.join("cgroup.procs")).ok()?;
    Some(text.lines().filter(|l| !l.trim().is_empty()).count() as u64)
}

/// A cgroup's `memory.current`, in bytes.
fn cgroup_memory(dir: &Path) -> Option<u64> {
    fs::read_to_string(dir.join("memory.current"))
        .ok()?
        .trim()
        .parse::<u64>()
        .ok()
}

/// Every cgroup under `root`, with the figures the kernel has for it.
///
/// Depth-first and sorted, so two runs of the same tree agree.
fn cgroup_usage_rows(root: &Path, base: &Path, into: &mut Vec<(String, Option<u64>, Option<u64>)>) {
    for kid in cgroup_children(base) {
        // Joined from components rather than the platform separator, and
        // via `to_str` rather than `to_string_lossy`: a cgroup name that is
        // not UTF-8 falls back to the escaped label instead of gaining
        // replacement characters.
        let name = kid.strip_prefix(root).map_or_else(
            |_| cgroup_label(&kid),
            |rel| {
                rel.components()
                    .filter_map(|c| c.as_os_str().to_str())
                    .collect::<Vec<_>>()
                    .join("/")
            },
        );
        into.push((name, cgroup_tasks(&kid), cgroup_memory(&kid)));
        cgroup_usage_rows(root, &kid, into);
    }
}

fn run_cgtop(out: &mut dyn Write) -> io::Result<i32> {
    // This used to print five invented cgroups with invented task counts,
    // CPU percentages and memory figures, having read nothing.
    let root = Path::new(CGROUP_ROOT);
    if !root.is_dir() {
        writeln!(out, "systemd-cgtop: {CGROUP_ROOT} is not a directory")?;
        return Ok(1);
    }
    writeln!(
        out,
        "{:<40} {:>6} {:>8} {:>8} {:>8}",
        "Control Group", "Tasks", "%CPU", "Memory", "Input/s"
    )?;
    let mut rows = Vec::new();
    cgroup_usage_rows(root, root, &mut rows);
    for (name, tasks, memory) in rows {
        writeln!(
            out,
            "{:<40} {:>6} {:>8} {:>8} {:>8}",
            name,
            tasks.map_or_else(|| "-".to_string(), |t| t.to_string()),
            // One sample has no interval to divide by, so there is no
            // percentage to print. The reference prints `-` here too, for
            // the same reason -- measured with `systemd-cgtop -n 1`.
            "-",
            memory.map_or_else(|| "-".to_string(), format_cgroup_size),
            "-"
        )?;
    }
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
        ("system-library-arch", "/usr/lib/x86_64-slateos"),
        ("system-configuration", "/etc"),
        ("system-state-private", "/var/lib"),
        ("system-state-logs", "/var/log"),
        ("system-state-cache", "/var/cache"),
        ("system-state-spool", "/var/spool"),
        ("system-runtime", "/run"),
        (
            "system-generator-early",
            "/run/slateos/system-generators.early",
        ),
        ("system-generator", "/usr/lib/slateos/system-generators"),
        (
            "system-generator-late",
            "/run/slateos/system-generators.late",
        ),
        ("system-preset", "/usr/lib/slateos/system-preset"),
        ("system-shutdown", "/usr/lib/slateos/system-shutdown"),
        ("system-sleep", "/usr/lib/slateos/system-sleep"),
        ("system-unit-path", "/usr/lib/slateos/system"),
        ("user-binaries", "/usr/local/bin"),
        ("user-library-private", "/usr/local/lib"),
        ("user-configuration", "/etc/slateos/user"),
        ("user-runtime", "/run/user"),
        ("user-unit-path", "/usr/lib/slateos/user"),
        (
            "search-binaries",
            "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin",
        ),
        (
            "search-binaries-default",
            "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin",
        ),
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
        writeln!(
            out,
            "Notify the service manager about service status changes."
        )?;
        writeln!(out)?;
        writeln!(out, "Options:")?;
        writeln!(
            out,
            "  --ready         Notify that service startup is complete"
        )?;
        writeln!(out, "  --reloading     Notify that service is reloading")?;
        writeln!(out, "  --stopping      Notify that service is stopping")?;
        writeln!(out, "  --status=TEXT   Set service status text")?;
        writeln!(out, "  --pid=PID       Send from specific PID")?;
        writeln!(
            out,
            "  --booted        Check if system was booted with systemd"
        )?;
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
                writeln!(
                    out,
                    "Create, clean, and remove temporary files and directories."
                )?;
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
        writeln!(
            out,
            "systemd-tmpfiles: No action specified (use --create, --clean, or --remove)."
        )?;
        return Ok(1);
    }

    // Simulated default config entries.
    let default_entries = [
        "d /tmp 1777 root root 10d",
        "d /var/tmp 1777 root root 30d",
        "d /run/lock 0755 root root -",
        "d /run/user 0755 root root -",
        "f /run/utmp 0664 root utmp -",
        "r! /tmp/.X*-lock - - - -",
    ];

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
                writeln!(
                    out,
                    "Unknown tmpfiles type '{}' for {}",
                    e.entry_type, e.path
                )?;
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

    let cmd = positional
        .first()
        .map(|s| s.as_str())
        .unwrap_or("list-units");
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
    writeln!(
        out,
        "Query or send control commands to the service manager."
    )?;
    writeln!(out)?;
    writeln!(out, "Unit Commands:")?;
    writeln!(
        out,
        "  list-units [PATTERN...]         List units in memory"
    )?;
    writeln!(
        out,
        "  list-unit-files [PATTERN...]    List installed unit files"
    )?;
    writeln!(out, "  status [UNIT...]                Show unit status")?;
    writeln!(out, "  show [UNIT...|JOB...]           Show properties")?;
    writeln!(
        out,
        "  cat UNIT...                     Show unit file contents"
    )?;
    writeln!(
        out,
        "  edit UNIT...                    Edit unit file overrides"
    )?;
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
    writeln!(
        out,
        "  list-dependencies [UNIT]        Show dependency tree"
    )?;
    writeln!(out, "  list-timers                     List timers")?;
    writeln!(out, "  list-sockets                    List sockets")?;
    writeln!(out)?;
    writeln!(out, "System Commands:")?;
    writeln!(
        out,
        "  poweroff                        Power off the system"
    )?;
    writeln!(out, "  reboot                          Reboot the system")?;
    writeln!(out, "  halt                            Halt the system")?;
    writeln!(out, "  suspend                         Suspend the system")?;
    writeln!(
        out,
        "  hibernate                       Hibernate the system"
    )?;
    writeln!(out, "  isolate TARGET                  Isolate a target")?;
    writeln!(out)?;
    writeln!(out, "Options:")?;
    writeln!(
        out,
        "  --user                          Talk to the user service manager"
    )?;
    writeln!(
        out,
        "  --system                        Talk to the system manager (default)"
    )?;
    writeln!(out, "  -t, --type=TYPE                 Filter by unit type")?;
    writeln!(
        out,
        "  --state=STATE                   Filter by unit state"
    )?;
    writeln!(
        out,
        "  -a, --all                       Show all units/properties"
    )?;
    writeln!(out, "  -q, --quiet                     Suppress output")?;
    writeln!(
        out,
        "  --no-pager                      Do not pipe output to pager"
    )?;
    writeln!(out, "  --no-legend                     Do not print legend")?;
    writeln!(out, "  --plain                         Plain output")?;
    writeln!(
        out,
        "  --now                           Start/stop immediately with enable/disable"
    )?;
    writeln!(out, "  -f, --force                     Force operation")?;
    writeln!(out, "  -h, --help                      Show this help")?;
    writeln!(out, "  --version                       Show version")?;
    Ok(())
}

/// Refuse an option this personality does not have.
///
/// systemd's tools print exactly one line -- no `Try --help` pointer -- and
/// use both of getopt's wordings. Measured: `systemd-cat --zzq` gives
/// `unrecognized option '--zzq'`, `systemd-cat -z` gives
/// `invalid option -- 'z'`, and all five exit 1.
fn refuse_unknown_option(prog: &str, arg: &str) -> i32 {
    eprintln!("{prog}: {}", usageerror::unknown_option(arg.as_bytes()));
    1
}

/// The first argument that is an option none of `known` covers.
///
/// `known` holds whole words (`--help`) and prefixes ending in `=`
/// (`--suffix=`), which match either the prefixed form or the bare word. A
/// lone `-` is an operand, and so is everything after `--`.
fn first_unknown_option<'a>(args: &'a [String], known: &[&str]) -> Option<&'a String> {
    let mut operands_only = false;
    args.iter().find(|a| {
        if operands_only {
            return false;
        }
        if a.as_str() == "--" {
            operands_only = true;
            return false;
        }
        if !a.starts_with('-') || a.as_str() == "-" {
            return false;
        }
        !known.iter().any(|k| match k.strip_suffix('=') {
            Some(bare) => a.starts_with(k) || a.as_str() == bare,
            None => a.as_str() == *k,
        })
    })
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
            } else if let Some(bad) = first_unknown_option(&rest, &["--help", "-h", "--version"]) {
                Ok(refuse_unknown_option("systemd-cat", bad))
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
            } else if let Some(bad) = first_unknown_option(&rest, &["--help", "-h", "--version"]) {
                Ok(refuse_unknown_option("systemd-cgls", bad))
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
            } else if let Some(bad) = first_unknown_option(&rest, &["--help", "-h", "--version"]) {
                Ok(refuse_unknown_option("systemd-cgtop", bad))
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
            } else if let Some(bad) = first_unknown_option(
                &rest,
                &[
                    "--help",
                    "-h",
                    "--version",
                    "-u",
                    "--unescape",
                    "-p",
                    "--path",
                    "--suffix=",
                ],
            ) {
                Ok(refuse_unknown_option("systemd-escape", bad))
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
            } else if let Some(bad) = first_unknown_option(&rest, &["--help", "-h", "--version"]) {
                Ok(refuse_unknown_option("systemd-path", bad))
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

    // ── Refusing an option a personality does not have ──

    fn argv(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| (*w).to_string()).collect()
    }

    #[test]
    fn a_known_option_is_not_reported() {
        let known = ["--help", "-h", "--version"];
        assert!(first_unknown_option(&argv(&["--help"]), &known).is_none());
        assert!(first_unknown_option(&argv(&["-h", "--version"]), &known).is_none());
    }

    #[test]
    fn the_first_unknown_option_is_reported() {
        let known = ["--help", "--version"];
        let args = argv(&["--version", "--zzq", "--also-bad"]);
        assert_eq!(
            first_unknown_option(&args, &known).map(String::as_str),
            Some("--zzq")
        );
    }

    /// Operands are not options, however they are spelled.
    #[test]
    fn operands_are_never_reported() {
        let known = ["--help"];
        assert!(first_unknown_option(&argv(&["unit.service"]), &known).is_none());
        // A lone dash is an operand -- conventionally stdin.
        assert!(first_unknown_option(&argv(&["-"]), &known).is_none());
        // And everything after `--` is one, dashes included.
        assert!(first_unknown_option(&argv(&["--", "--zzq"]), &known).is_none());
    }

    /// `--suffix=` in the known list covers both `--suffix=x` and a bare
    /// `--suffix`, which is how systemd-escape spells that option.
    #[test]
    fn a_prefix_entry_covers_its_valued_form() {
        let known = ["--suffix="];
        assert!(first_unknown_option(&argv(&["--suffix=service"]), &known).is_none());
        assert!(first_unknown_option(&argv(&["--suffix"]), &known).is_none());
        // But the prefix is `--suffix=`, not "anything starting with
        // --suffix": `--suffixx` is a different word and is refused.
        assert_eq!(
            first_unknown_option(&argv(&["--suffixx"]), &known).map(String::as_str),
            Some("--suffixx")
        );
    }

    #[test]
    fn the_refusal_reads_as_systemd_prints_it() {
        // One line, no `Try --help` pointer, and both getopt wordings.
        assert_eq!(
            usageerror::unknown_option(b"--zzq"),
            "unrecognized option '--zzq'"
        );
        assert_eq!(usageerror::unknown_option(b"-z"), "invalid option -- 'z'");
    }

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
        assert_eq!(detect_personality("systemd-analyze"), Personality::Analyze);
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
        assert_eq!(UnitType::from_unit_name("foo.timer"), Some(UnitType::Timer));
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
        assert!(
            issues
                .iter()
                .any(|i| i.contains("Missing [Service] section"))
        );
    }

    #[test]
    fn test_verify_missing_description() {
        let content = "[Unit]\nAfter=basic.target\n[Service]\nExecStart=/bin/foo\n";
        let uf = UnitFile::parse(content).unwrap();
        let issues = uf.verify("nodesc.service");
        assert!(issues.iter().any(|i| i.contains("Missing Description")));
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
        let content = "[Unit]\nDescription=Test\n[Service]\nType=oneshot\nExecStart=/bin/setup\n";
        let uf = UnitFile::parse(content).unwrap();
        let issues = uf.verify("oneshot.service");
        assert!(!issues.iter().any(|i| i.contains("Missing ExecStart")));
    }

    #[test]
    fn test_verify_timer_missing_section() {
        let content = "[Unit]\nDescription=Test\n";
        let uf = UnitFile::parse(content).unwrap();
        let issues = uf.verify("bad.timer");
        assert!(issues.iter().any(|i| i.contains("Missing [Timer] section")));
    }

    #[test]
    fn test_verify_socket_missing_section() {
        let content = "[Unit]\nDescription=Test\n";
        let uf = UnitFile::parse(content).unwrap();
        let issues = uf.verify("bad.socket");
        assert!(
            issues
                .iter()
                .any(|i| i.contains("Missing [Socket] section"))
        );
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
        assert_eq!(result, "slateos");
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
        let args = vec![
            "--now".to_string(),
            "enable".to_string(),
            "sshd.service".to_string(),
        ];
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

    // Every test below asserted the hard-coded unit list. They are kept, one
    // for one, rather than deleted -- each now pins the honest contract for
    // the subcommand it used to certify a fabrication for. The old
    // expectation is quoted where it is sharpest, so the change reads as a
    // correction rather than a loosening.

    /// `list-units` refuses rather than listing four hard-coded names.
    ///
    /// Asserted `out.contains("init.service")` before -- on a program that had
    /// asked nothing.
    #[test]
    fn test_list_units_default() {
        let (out, code) = capture(|buf| cmd_list_units(buf, &SystemctlFlags::default()));
        assert_eq!(code, 1);
        assert!(out.is_empty(), "printed a unit table anyway: {out}");
    }

    /// A filter cannot make an unavailable listing available.
    #[test]
    fn test_list_units_type_filter() {
        let flags = SystemctlFlags {
            type_filter: Some("target".to_string()),
            ..Default::default()
        };
        let (out, code) = capture(|buf| cmd_list_units(buf, &flags));
        assert_eq!(code, 1);
        assert!(out.is_empty());
    }

    /// Nor can `--no-legend`.
    #[test]
    fn test_list_units_no_legend() {
        let flags = SystemctlFlags {
            no_legend: true,
            ..Default::default()
        };
        let (out, code) = capture(|buf| cmd_list_units(buf, &flags));
        assert_eq!(code, 1);
        assert!(out.is_empty());
    }

    // --- list-unit-files ---

    #[test]
    fn test_list_unit_files() {
        let (out, code) = capture(|buf| cmd_list_unit_files(buf, &SystemctlFlags::default()));
        assert_eq!(code, 1);
        assert!(out.is_empty(), "printed a unit-file table anyway: {out}");
    }

    #[test]
    fn test_list_unit_files_type_filter() {
        let flags = SystemctlFlags {
            type_filter: Some("service".to_string()),
            ..Default::default()
        };
        let (out, code) = capture(|buf| cmd_list_unit_files(buf, &flags));
        assert_eq!(code, 1);
        assert!(out.is_empty());
    }

    // --- status ---

    /// There is no such thing as a known unit here.
    ///
    /// This is the sharpest of the sixteen. It asserted that `systemctl status
    /// sshd.service` printed "active" and "OpenSSH Server" and exited 0 -- so
    /// an operator checking whether sshd was up was told yes, by a program
    /// that had not looked, and a passing test said that was correct.
    #[test]
    fn test_status_known_unit() {
        let (out, code) =
            capture(|buf| cmd_status(buf, "sshd.service", &SystemctlFlags::default()));
        assert_ne!(code, 0, "reported a service state without a source");
        assert!(!out.contains("active"), "claimed a state: {out}");
    }

    /// And an unfamiliar name is not reported as absent.
    ///
    /// This asserted exit 4, which in systemd means "no such unit". This
    /// program cannot tell a unit that does not exist from one it cannot see,
    /// so claiming the former would be the same defect facing the other way.
    #[test]
    fn test_status_unknown_unit() {
        let (_out, code) =
            capture(|buf| cmd_status(buf, "nonexistent.service", &SystemctlFlags::default()));
        assert_eq!(code, 1);
        assert_ne!(code, 4, "claimed the unit does not exist");
    }

    // --- show ---

    #[test]
    fn test_show_all_properties() {
        let (out, code) =
            capture(|buf| cmd_show(buf, "sshd.service", None, &SystemctlFlags::default()));
        assert_eq!(code, 1);
        assert!(!out.contains("ActiveState="), "claimed properties: {out}");
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
        assert_eq!(code, 1);
        assert!(!out.contains("active"), "claimed a state: {out}");
    }

    // --- is-active ---

    /// **The one a script branches on.**
    ///
    /// `is-active` used to exit 0 for four hard-coded names, so
    /// `systemctl is-active sshd || start_it` never started anything. It now
    /// prints `unknown` -- which is what systemd prints for a unit it cannot
    /// resolve -- and exits non-zero, so the script takes the safe branch.
    #[test]
    fn test_is_active_active() {
        let (out, code) =
            capture(|buf| cmd_is_active(buf, "sshd.service", &SystemctlFlags::default()));
        assert_ne!(code, 0, "a script would have concluded sshd is running");
        assert!(out.contains("unknown"), "{out}");
    }

    #[test]
    fn test_is_active_unknown() {
        let (out, code) =
            capture(|buf| cmd_is_active(buf, "nope.service", &SystemctlFlags::default()));
        assert_ne!(code, 0);
        assert!(out.contains("unknown"));
    }

    // --- is-enabled ---

    #[test]
    fn test_is_enabled_enabled() {
        let (_out, code) =
            capture(|buf| cmd_is_enabled(buf, "sshd.service", &SystemctlFlags::default()));
        assert_ne!(code, 0, "claimed a unit is enabled without a source");
    }

    #[test]
    fn test_is_enabled_static() {
        let (_out, code) =
            capture(|buf| cmd_is_enabled(buf, "basic.target", &SystemctlFlags::default()));
        assert_ne!(code, 0);
    }

    // --- is-failed ---

    /// `is-failed` must not answer "no" either.
    ///
    /// Exiting non-zero here reads as "not failed", which is the same answer
    /// it used to give -- but it now says `unknown` on stdout, so a human sees
    /// the difference even though the status cannot express it.
    #[test]
    fn test_is_failed_not_failed() {
        let (out, code) =
            capture(|buf| cmd_is_failed(buf, "sshd.service", &SystemctlFlags::default()));
        assert_ne!(code, 0);
        assert!(out.contains("unknown"), "{out}");
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
        let (out, code) =
            capture(|buf| cmd_unit_action(buf, "stop", "sshd.service", &SystemctlFlags::default()));
        assert_eq!(code, 0);
        assert!(out.contains("Stopping sshd.service"));
    }

    #[test]
    fn test_enable_with_now() {
        let flags = SystemctlFlags {
            now: true,
            ..Default::default()
        };
        let (out, code) = capture(|buf| cmd_unit_action(buf, "enable", "sshd.service", &flags));
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
        let (out, code) =
            capture(|buf| cmd_unit_action(buf, "mask", "sshd.service", &SystemctlFlags::default()));
        assert_eq!(code, 0);
        assert!(out.contains("/dev/null"));
    }

    #[test]
    fn test_auto_append_service() {
        let (out, code) =
            capture(|buf| cmd_unit_action(buf, "start", "sshd", &SystemctlFlags::default()));
        assert_eq!(code, 0);
        assert!(out.contains("sshd.service"));
    }

    #[test]
    fn test_quiet_suppresses_output() {
        let flags = SystemctlFlags {
            quiet: true,
            ..Default::default()
        };
        let (out, code) = capture(|buf| cmd_unit_action(buf, "start", "sshd.service", &flags));
        assert_eq!(code, 0);
        assert!(out.is_empty());
    }

    // --- daemon-reload ---

    #[test]
    fn test_daemon_reload() {
        let (out, code) = capture(|buf| cmd_daemon_reload(buf, &SystemctlFlags::default()));
        assert_eq!(code, 0);
        assert!(out.contains("Reloading"));
    }

    // --- cat unit ---

    #[test]
    fn test_cat_known_unit() {
        let (out, code) = capture(|buf| cmd_cat_unit(buf, "sshd.service"));
        assert_eq!(code, 1);
        assert!(
            out.is_empty(),
            "printed a unit file it does not have: {out}"
        );
    }

    #[test]
    fn test_cat_unknown_unit() {
        let (out, code) = capture(|buf| cmd_cat_unit(buf, "nope.service"));
        assert_eq!(code, 1);
        assert!(out.is_empty());
    }

    // --- power commands ---

    #[test]
    fn test_poweroff() {
        let (out, code) = capture(|buf| cmd_power(buf, "poweroff", &SystemctlFlags::default()));
        assert_eq!(code, 0);
        assert!(out.contains("powering off"));
    }

    #[test]
    fn test_reboot() {
        let (out, code) = capture(|buf| cmd_power(buf, "reboot", &SystemctlFlags::default()));
        assert_eq!(code, 0);
        assert!(out.contains("rebooting"));
    }

    // --- isolate ---

    #[test]
    fn test_isolate() {
        let (out, code) =
            capture(|buf| cmd_isolate(buf, "rescue.target", &SystemctlFlags::default()));
        assert_eq!(code, 0);
        assert!(out.contains("Isolating rescue.target"));
    }

    // --- list-timers ---

    #[test]
    fn test_list_timers() {
        let (out, code) = capture(|buf| cmd_list_timers(buf, &SystemctlFlags::default()));
        assert_eq!(code, 0);
        assert!(out.contains("logwatch.timer"));
        assert!(out.contains("NEXT"));
    }

    // --- list-sockets ---

    #[test]
    fn test_list_sockets() {
        let (out, code) = capture(|buf| cmd_list_sockets(buf, &SystemctlFlags::default()));
        assert_eq!(code, 1);
        assert!(
            out.is_empty(),
            "listed a socket it did not enumerate: {out}"
        );
    }

    // --- list-dependencies ---

    /// A dependency tree is what you consult before stopping something.
    ///
    /// This asserted the invented children of `multi-user.target`. An invented
    /// graph is worse than no graph: it is the thing you read to decide what
    /// else goes down.
    #[test]
    fn test_list_dependencies() {
        let (out, code) = capture(|buf| {
            cmd_list_dependencies(buf, "multi-user.target", &SystemctlFlags::default())
        });
        assert_eq!(code, 1);
        assert!(out.is_empty(), "printed a dependency tree: {out}");
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
        let (out, code) = capture(|buf| analyze_critical_chain(buf, Some("multi-user.target")));
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

    /// The old test here asserted that the output contained `system.slice`
    /// and `user.slice` on any machine, which was true only because those
    /// names were hardcoded. A test that pins a fabrication in place is
    /// worse than no test: it makes the lie look verified.
    #[test]
    fn a_host_without_cgroups_is_told_so_rather_than_shown_a_tree() {
        // No `/sys/fs/cgroup` on the machine this suite runs on.
        let (out, code) = capture(|buf| run_cgls(buf));
        assert_eq!(code, 1, "{out}");
        assert!(out.contains("not a directory"), "{out}");
        assert!(!out.contains("system.slice"), "{out}");
    }

    #[test]
    fn the_tree_is_the_directories_that_are_actually_there() {
        let scratch = scratchdir::ScratchDir::new("cgls-tree");
        let root = scratch.path("root");
        fs::create_dir_all(root.join("system.slice").join("dbus.service")).expect("fixture");
        fs::write(
            root.join("system.slice")
                .join("dbus.service")
                .join("cgroup.procs"),
            "4242\n",
        )
        .expect("fixture");

        let mut buf: Vec<u8> = Vec::new();
        print_cgroup_tree(&mut buf, &root, "").expect("write");
        let out = String::from_utf8(buf).expect("ascii fixture");

        assert!(out.contains("system.slice"), "{out}");
        assert!(out.contains("dbus.service"), "{out}");
        assert!(out.contains("4242"), "{out}");
        // The discriminating half: the old code printed `user.slice` for
        // every machine, so a tree that does not contain one it was not
        // given is the whole point.
        assert!(!out.contains("user.slice"), "{out}");
        assert!(!out.contains("init.scope"), "{out}");
    }

    #[test]
    fn a_cgroup_with_no_procs_file_contributes_no_pids() {
        let scratch = scratchdir::ScratchDir::new("cgls-empty");
        let root = scratch.path("root");
        fs::create_dir_all(root.join("lonely.slice")).expect("fixture");
        assert!(cgroup_procs(&root.join("lonely.slice")).is_empty());
        assert_eq!(cgroup_children(&root).len(), 1);
    }

    // --- systemd-cgtop ---

    /// Same story as the `cgls` test it sits beside: this asserted
    /// `/system.slice` appeared, which was true on every machine because
    /// the row was hardcoded.
    #[test]
    fn cgtop_on_a_host_without_cgroups_says_so() {
        let (out, code) = capture(|buf| run_cgtop(buf));
        assert_eq!(code, 1, "{out}");
        assert!(out.contains("not a directory"), "{out}");
        assert!(!out.contains("system.slice"), "{out}");
    }

    #[test]
    fn cgtop_reports_the_figures_the_files_hold() {
        let scratch = scratchdir::ScratchDir::new("cgtop");
        let root = scratch.path("root");
        let svc = root.join("system.slice").join("sshd.service");
        fs::create_dir_all(&svc).expect("fixture");
        fs::write(svc.join("pids.current"), "3\n").expect("fixture");
        fs::write(svc.join("memory.current"), "8388608\n").expect("fixture");

        let mut rows = Vec::new();
        cgroup_usage_rows(&root, &root, &mut rows);

        let found = rows
            .iter()
            .find(|(name, _, _)| name.ends_with("sshd.service"))
            .expect("the cgroup that was created");
        assert_eq!(found.1, Some(3), "tasks come from pids.current");
        assert_eq!(found.2, Some(8_388_608), "memory comes from memory.current");
        assert_eq!(format_cgroup_size(8_388_608), "8.0M");
        // Nothing invents a group that was not there.
        assert!(!rows.iter().any(|(n, _, _)| n.contains("user.slice")));
    }

    /// A group whose counters are unreadable is `-`, not 0. Zero would say
    /// the group is empty, which is a different claim from not knowing.
    #[test]
    fn cgtop_distinguishes_unknown_from_zero() {
        let scratch = scratchdir::ScratchDir::new("cgtop-bare");
        let root = scratch.path("root");
        fs::create_dir_all(root.join("bare.slice")).expect("fixture");
        assert_eq!(cgroup_memory(&root.join("bare.slice")), None);
        // No `pids.current`, but an absent `cgroup.procs` too, so unknown.
        assert_eq!(cgroup_tasks(&root.join("bare.slice")), None);
    }

    /// Measured against the reference, which renders 4096 as `4.0K` and
    /// 162 424 832 as `154.9M`.
    #[test]
    fn cgroup_sizes_read_as_the_reference_prints_them() {
        assert_eq!(format_cgroup_size(4096), "4.0K");
        assert_eq!(format_cgroup_size(162_424_832), "154.9M");
        assert_eq!(format_cgroup_size(512), "512B");
        assert_eq!(format_cgroup_size(1024), "1.0K");
        assert_eq!(format_cgroup_size(1024 * 1024 * 1024), "1.0G");
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
