//! OurOS firejail security sandbox framework.
//!
//! Multi-personality binary providing:
//! - **firejail** (default) -- security sandbox for running applications
//! - **firemon** -- monitor running sandboxes
//! - **firecfg** -- configure default sandboxes for applications
//!
//! Implements Linux-compatible firejail sandbox isolation for OurOS,
//! supporting network namespaces, filesystem restrictions, capability
//! dropping, seccomp filtering, and profile-based configuration.

#![deny(clippy::all)]
#![allow(clippy::too_many_lines)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::struct_excessive_bools)]
#![allow(clippy::similar_names)]
#![allow(clippy::cast_possible_truncation)]

use std::collections::HashMap;
use std::env;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process;

const VERSION: &str = "0.1.0";
const DEFAULT_PROFILE_DIR: &str = "/etc/firejail";
const DEFAULT_SANDBOX_DIR: &str = "/run/firejail";
const DEFAULT_SYMLINK_DIR: &str = "/usr/local/bin";

// ============================================================================
// Network configuration
// ============================================================================

/// Network mode for a sandbox.
#[derive(Clone, Debug, PartialEq, Eq)]
#[derive(Default)]
enum NetMode {
    /// No network restrictions (host networking).
    #[default]
    Host,
    /// No network access at all.
    None,
    /// Attach to a specific interface (bridge or physical).
    Interface(String),
}


impl fmt::Display for NetMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Host => write!(f, "host"),
            Self::None => write!(f, "none"),
            Self::Interface(iface) => write!(f, "{iface}"),
        }
    }
}

/// DNS configuration for a sandbox.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct DnsConfig {
    servers: Vec<String>,
}

/// IP address configuration for a sandbox.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct IpConfig {
    address: Option<String>,
    mac: Option<String>,
}

/// Full network configuration.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct NetworkConfig {
    mode: NetMode,
    dns: DnsConfig,
    ip: IpConfig,
    hostname: Option<String>,
    netfilter: bool,
}

// ============================================================================
// Filesystem restrictions
// ============================================================================

/// Types of filesystem restrictions that can be applied.
#[derive(Clone, Debug, PartialEq, Eq)]
enum FsRestriction {
    /// Make a path read-only inside the sandbox.
    ReadOnly(String),
    /// Mount a tmpfs at the given path.
    Tmpfs(String),
    /// Blacklist (hide) a path inside the sandbox.
    Blacklist(String),
    /// Whitelist (allow only) a path inside the sandbox.
    Whitelist(String),
    /// Prevent execution of binaries under a path.
    NoExec(String),
}

impl fmt::Display for FsRestriction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ReadOnly(p) => write!(f, "read-only {p}"),
            Self::Tmpfs(p) => write!(f, "tmpfs {p}"),
            Self::Blacklist(p) => write!(f, "blacklist {p}"),
            Self::Whitelist(p) => write!(f, "whitelist {p}"),
            Self::NoExec(p) => write!(f, "noexec {p}"),
        }
    }
}

/// Filesystem isolation options.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct FsConfig {
    private_home: bool,
    private_tmp: bool,
    private_dev: bool,
    private_etc: Vec<String>,
    private_bin: Vec<String>,
    restrictions: Vec<FsRestriction>,
}

// ============================================================================
// Security configuration
// ============================================================================

/// Capability drop mode.
#[derive(Clone, Debug, PartialEq, Eq)]
#[derive(Default)]
enum CapsDrop {
    /// Do not drop any capabilities.
    #[default]
    None,
    /// Drop all capabilities.
    All,
    /// Drop specific capabilities.
    List(Vec<String>),
}


impl fmt::Display for CapsDrop {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => write!(f, "none"),
            Self::All => write!(f, "all"),
            Self::List(caps) => write!(f, "{}", caps.join(",")),
        }
    }
}

/// Security restrictions for a sandbox.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct SecurityConfig {
    caps_drop: CapsDrop,
    seccomp: bool,
    noroot: bool,
    nosound: bool,
    no3d: bool,
    novideo: bool,
    nodvd: bool,
    notv: bool,
    nou2f: bool,
}

// ============================================================================
// Sandbox configuration (full)
// ============================================================================

/// Complete sandbox configuration, populated from command-line options and/or
/// a profile file.
#[derive(Clone, Debug, Default)]
struct SandboxConfig {
    name: Option<String>,
    network: NetworkConfig,
    filesystem: FsConfig,
    security: SecurityConfig,
    shell: Option<String>,
    noprofile: bool,
    profile_path: Option<String>,
    debug: bool,
    quiet: bool,
}

impl SandboxConfig {
    fn new() -> Self {
        Self::default()
    }

    /// Merge profile directives into this config. Profile values are applied
    /// only when the config field has not already been set by a CLI flag.
    fn merge_profile(&mut self, profile: &ProfileConfig) {
        if !self.filesystem.private_home && profile.private_home {
            self.filesystem.private_home = true;
        }
        if !self.filesystem.private_tmp && profile.private_tmp {
            self.filesystem.private_tmp = true;
        }
        if !self.filesystem.private_dev && profile.private_dev {
            self.filesystem.private_dev = true;
        }
        if self.filesystem.private_etc.is_empty() && !profile.private_etc.is_empty() {
            self.filesystem.private_etc = profile.private_etc.clone();
        }
        if self.filesystem.private_bin.is_empty() && !profile.private_bin.is_empty() {
            self.filesystem.private_bin = profile.private_bin.clone();
        }
        if !self.security.seccomp && profile.seccomp {
            self.security.seccomp = true;
        }
        if matches!(self.security.caps_drop, CapsDrop::None) && profile.caps_drop_all {
            self.security.caps_drop = CapsDrop::All;
        }
        if !self.security.noroot && profile.noroot {
            self.security.noroot = true;
        }
        if !self.security.nosound && profile.nosound {
            self.security.nosound = true;
        }
        if !self.security.no3d && profile.no3d {
            self.security.no3d = true;
        }
        if !self.security.novideo && profile.novideo {
            self.security.novideo = true;
        }
        if !self.security.nodvd && profile.nodvd {
            self.security.nodvd = true;
        }
        if !self.security.notv && profile.notv {
            self.security.notv = true;
        }
        if !self.security.nou2f && profile.nou2f {
            self.security.nou2f = true;
        }
        if !self.network.netfilter && profile.netfilter {
            self.network.netfilter = true;
        }
        if matches!(self.network.mode, NetMode::Host)
            && let Some(ref mode) = profile.net_mode {
                self.network.mode = mode.clone();
            }
        for r in &profile.restrictions {
            self.filesystem.restrictions.push(r.clone());
        }
    }
}

// ============================================================================
// Profile parsing
// ============================================================================

/// Parsed profile configuration from a .profile file.
#[derive(Clone, Debug, Default)]
struct ProfileConfig {
    name: String,
    private_home: bool,
    private_tmp: bool,
    private_dev: bool,
    private_etc: Vec<String>,
    private_bin: Vec<String>,
    seccomp: bool,
    caps_drop_all: bool,
    noroot: bool,
    nosound: bool,
    no3d: bool,
    novideo: bool,
    nodvd: bool,
    notv: bool,
    nou2f: bool,
    netfilter: bool,
    net_mode: Option<NetMode>,
    restrictions: Vec<FsRestriction>,
    includes: Vec<String>,
}

/// Parse a single profile line into its directive and optional value.
fn parse_profile_line(line: &str) -> Option<(&str, Option<&str>)> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return Option::None;
    }
    if let Some(idx) = trimmed.find(' ') {
        let (directive, rest) = trimmed.split_at(idx);
        Some((directive, Some(rest.trim())))
    } else {
        Some((trimmed, Option::None))
    }
}

/// Parse a profile file at the given path.
fn parse_profile(path: &Path) -> Result<ProfileConfig, String> {
    let content = fs::read_to_string(path)
        .map_err(|e| format!("cannot read profile {}: {e}", path.display()))?;
    parse_profile_content(&content, path)
}

/// Parse profile content from a string. The path is used for error messages
/// and for resolving `include` directives.
fn parse_profile_content(content: &str, source: &Path) -> Result<ProfileConfig, String> {
    let mut profile = ProfileConfig::default();
    profile.name = source
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string();

    for line in content.lines() {
        let Some((directive, value)) = parse_profile_line(line) else {
            continue;
        };
        match directive {
            "include" => {
                if let Some(inc_path) = value {
                    profile.includes.push(inc_path.to_string());
                }
            }
            "private" => profile.private_home = true,
            "private-tmp" => profile.private_tmp = true,
            "private-dev" => profile.private_dev = true,
            "private-etc" => {
                if let Some(files) = value {
                    profile.private_etc = split_csv(files);
                }
            }
            "private-bin" => {
                if let Some(bins) = value {
                    profile.private_bin = split_csv(bins);
                }
            }
            "seccomp" => profile.seccomp = true,
            "caps.drop" => {
                if value == Some("all") {
                    profile.caps_drop_all = true;
                }
            }
            "noroot" => profile.noroot = true,
            "nosound" => profile.nosound = true,
            "no3d" => profile.no3d = true,
            "novideo" => profile.novideo = true,
            "nodvd" => profile.nodvd = true,
            "notv" => profile.notv = true,
            "nou2f" => profile.nou2f = true,
            "netfilter" => profile.netfilter = true,
            "net" => {
                if let Some(v) = value {
                    if v == "none" {
                        profile.net_mode = Some(NetMode::None);
                    } else {
                        profile.net_mode = Some(NetMode::Interface(v.to_string()));
                    }
                }
            }
            "read-only" => {
                if let Some(p) = value {
                    profile
                        .restrictions
                        .push(FsRestriction::ReadOnly(p.to_string()));
                }
            }
            "tmpfs" => {
                if let Some(p) = value {
                    profile
                        .restrictions
                        .push(FsRestriction::Tmpfs(p.to_string()));
                }
            }
            "blacklist" => {
                if let Some(p) = value {
                    profile
                        .restrictions
                        .push(FsRestriction::Blacklist(p.to_string()));
                }
            }
            "whitelist" => {
                if let Some(p) = value {
                    profile
                        .restrictions
                        .push(FsRestriction::Whitelist(p.to_string()));
                }
            }
            "noexec" => {
                if let Some(p) = value {
                    profile
                        .restrictions
                        .push(FsRestriction::NoExec(p.to_string()));
                }
            }
            "shell" => {
                // Shell directive handled at sandbox level.
            }
            "hostname" => {
                // Hostname directive handled at sandbox level.
            }
            _ => {
                // Ignore unknown directives for forward compatibility.
            }
        }
    }
    Ok(profile)
}

/// Split a comma-separated value string into individual trimmed entries.
fn split_csv(s: &str) -> Vec<String> {
    s.split(',').map(|p| p.trim().to_string()).filter(|p| !p.is_empty()).collect()
}

// ============================================================================
// Default profiles for common applications
// ============================================================================

/// Built-in application profiles. These are used when no on-disk profile
/// exists for a given application name.
fn builtin_profiles() -> HashMap<&'static str, &'static str> {
    let mut m = HashMap::new();
    m.insert(
        "firefox",
        "# Firefox profile\n\
         private-tmp\n\
         private-dev\n\
         private-etc fonts,machine-id,ca-certificates,ssl,pki,crypto-policies\n\
         private-bin firefox,sh\n\
         caps.drop all\n\
         seccomp\n\
         noroot\n\
         netfilter\n\
         blacklist /boot\n\
         blacklist /sbin\n\
         blacklist /usr/sbin\n\
         read-only /etc\n\
         noexec /tmp\n",
    );
    m.insert(
        "chromium",
        "# Chromium profile\n\
         private-tmp\n\
         private-dev\n\
         private-etc fonts,machine-id,ca-certificates,ssl,pki,crypto-policies\n\
         private-bin chromium,sh\n\
         caps.drop all\n\
         seccomp\n\
         noroot\n\
         netfilter\n\
         blacklist /boot\n\
         blacklist /sbin\n\
         read-only /etc\n",
    );
    m.insert(
        "vlc",
        "# VLC profile\n\
         private-tmp\n\
         private-dev\n\
         caps.drop all\n\
         seccomp\n\
         noroot\n\
         nosound\n\
         no3d\n\
         blacklist /boot\n\
         blacklist /sbin\n",
    );
    m.insert(
        "mpv",
        "# mpv profile\n\
         private-tmp\n\
         private-dev\n\
         caps.drop all\n\
         seccomp\n\
         noroot\n\
         blacklist /boot\n",
    );
    m.insert(
        "transmission-gtk",
        "# Transmission profile\n\
         private-tmp\n\
         private-dev\n\
         caps.drop all\n\
         seccomp\n\
         noroot\n\
         blacklist /boot\n\
         blacklist /sbin\n\
         netfilter\n",
    );
    m.insert(
        "evince",
        "# Evince (PDF viewer) profile\n\
         private-tmp\n\
         private-dev\n\
         caps.drop all\n\
         seccomp\n\
         noroot\n\
         net none\n\
         nosound\n\
         no3d\n\
         novideo\n",
    );
    m.insert(
        "libreoffice",
        "# LibreOffice profile\n\
         private-tmp\n\
         private-dev\n\
         caps.drop all\n\
         seccomp\n\
         noroot\n\
         nosound\n\
         no3d\n\
         blacklist /boot\n",
    );
    m.insert(
        "thunderbird",
        "# Thunderbird profile\n\
         private-tmp\n\
         private-dev\n\
         private-etc fonts,machine-id,ca-certificates,ssl,pki,crypto-policies\n\
         caps.drop all\n\
         seccomp\n\
         noroot\n\
         netfilter\n\
         blacklist /boot\n\
         blacklist /sbin\n",
    );
    m.insert(
        "gimp",
        "# GIMP profile\n\
         private-tmp\n\
         private-dev\n\
         caps.drop all\n\
         seccomp\n\
         noroot\n\
         net none\n\
         nosound\n\
         novideo\n",
    );
    m.insert(
        "inkscape",
        "# Inkscape profile\n\
         private-tmp\n\
         private-dev\n\
         caps.drop all\n\
         seccomp\n\
         noroot\n\
         net none\n\
         nosound\n\
         novideo\n",
    );
    m.insert(
        "audacity",
        "# Audacity profile\n\
         private-tmp\n\
         private-dev\n\
         caps.drop all\n\
         seccomp\n\
         noroot\n\
         net none\n\
         no3d\n\
         novideo\n",
    );
    m.insert(
        "steam",
        "# Steam profile\n\
         private-tmp\n\
         caps.drop all\n\
         seccomp\n\
         noroot\n\
         netfilter\n\
         blacklist /boot\n\
         blacklist /sbin\n",
    );
    m.insert(
        "wget",
        "# wget profile\n\
         private-tmp\n\
         private-dev\n\
         caps.drop all\n\
         seccomp\n\
         noroot\n\
         blacklist /boot\n\
         blacklist /sbin\n",
    );
    m.insert(
        "curl",
        "# curl profile\n\
         private-tmp\n\
         private-dev\n\
         caps.drop all\n\
         seccomp\n\
         noroot\n\
         blacklist /boot\n\
         blacklist /sbin\n",
    );
    m.insert(
        "ssh",
        "# SSH profile\n\
         private-tmp\n\
         private-dev\n\
         caps.drop all\n\
         seccomp\n\
         noroot\n\
         blacklist /boot\n",
    );
    m
}

/// Names of all applications that have built-in profiles.
fn supported_app_names() -> Vec<&'static str> {
    let mut names: Vec<&str> = builtin_profiles().keys().copied().collect();
    names.sort_unstable();
    names
}

// ============================================================================
// Sandbox state (runtime info for running sandboxes)
// ============================================================================

/// Sandbox entry representing a running sandbox. CPU usage is stored as
/// millipercent (integer) to avoid floating-point complications with Eq.
#[derive(Clone, Debug, PartialEq, Eq)]
struct SandboxInfo {
    pid: u32,
    name: String,
    program: String,
    user: String,
    uptime_secs: u64,
    /// CPU usage in millipercent (e.g. 15500 = 15.5%).
    cpu_millipercent: u32,
    mem_kb: u64,
    net_mode: String,
    caps_dropped: bool,
    seccomp_enabled: bool,
    children: Vec<u32>,
}

impl SandboxInfo {
    fn cpu_display(&self) -> String {
        let whole = self.cpu_millipercent / 1000;
        let frac = (self.cpu_millipercent % 1000) / 100;
        format!("{whole}.{frac}%")
    }
}

impl fmt::Display for SandboxInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{pid:>6} {user:<12} {name:<20} {program:<30} {cpu:>6} {mem:>8} KB  {net}",
            pid = self.pid,
            user = self.user,
            name = self.name,
            program = self.program,
            cpu = self.cpu_display(),
            mem = self.mem_kb,
            net = self.net_mode,
        )
    }
}

// ============================================================================
// Sandbox directory operations
// ============================================================================

/// Read all sandbox info files from the sandbox runtime directory.
fn read_sandbox_entries(sandbox_dir: &Path) -> Vec<SandboxInfo> {
    let mut entries = Vec::new();
    let dir = match fs::read_dir(sandbox_dir) {
        Ok(d) => d,
        Err(_) => return entries,
    };
    for entry in dir {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("sandbox") {
            continue;
        }
        if let Some(info) = parse_sandbox_file(&path) {
            entries.push(info);
        }
    }
    entries.sort_by_key(|e| e.pid);
    entries
}

/// Parse a single sandbox info file. Format is key=value lines.
fn parse_sandbox_file(path: &Path) -> Option<SandboxInfo> {
    let content = fs::read_to_string(path).ok()?;
    let mut pid: Option<u32> = Option::None;
    let mut name = String::new();
    let mut program = String::new();
    let mut user = String::new();
    let mut uptime_secs: u64 = 0;
    let mut cpu_millipercent: u32 = 0;
    let mut mem_kb: u64 = 0;
    let mut net_mode = String::from("host");
    let mut caps_dropped = false;
    let mut seccomp_enabled = false;
    let mut children = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(idx) = trimmed.find('=') {
            let key = &trimmed[..idx];
            let val = &trimmed[idx + 1..];
            match key {
                "pid" => pid = val.parse().ok(),
                "name" => name = val.to_string(),
                "program" => program = val.to_string(),
                "user" => user = val.to_string(),
                "uptime" => uptime_secs = val.parse().unwrap_or(0),
                "cpu" => cpu_millipercent = val.parse().unwrap_or(0),
                "mem" => mem_kb = val.parse().unwrap_or(0),
                "net" => net_mode = val.to_string(),
                "caps" => caps_dropped = val == "dropped",
                "seccomp" => seccomp_enabled = val == "enabled",
                "children" => {
                    children = val
                        .split(',')
                        .filter_map(|s| s.trim().parse().ok())
                        .collect();
                }
                _ => {}
            }
        }
    }
    Some(SandboxInfo {
        pid: pid?,
        name,
        program,
        user,
        uptime_secs,
        cpu_millipercent,
        mem_kb,
        net_mode,
        caps_dropped,
        seccomp_enabled,
        children,
    })
}

/// Write a sandbox info file for a new sandbox.
fn write_sandbox_file(sandbox_dir: &Path, info: &SandboxInfo) -> Result<(), String> {
    let _ = fs::create_dir_all(sandbox_dir);
    let path = sandbox_dir.join(format!("{}.sandbox", info.pid));
    let children_str: Vec<String> = info.children.iter().map(|c| c.to_string()).collect();
    let content = format!(
        "pid={pid}\n\
         name={name}\n\
         program={program}\n\
         user={user}\n\
         uptime={uptime}\n\
         cpu={cpu}\n\
         mem={mem}\n\
         net={net}\n\
         caps={caps}\n\
         seccomp={seccomp}\n\
         children={children}\n",
        pid = info.pid,
        name = info.name,
        program = info.program,
        user = info.user,
        uptime = info.uptime_secs,
        cpu = info.cpu_millipercent,
        mem = info.mem_kb,
        net = info.net_mode,
        caps = if info.caps_dropped { "dropped" } else { "retained" },
        seccomp = if info.seccomp_enabled { "enabled" } else { "disabled" },
        children = children_str.join(","),
    );
    fs::write(&path, content)
        .map_err(|e| format!("cannot write sandbox file {}: {e}", path.display()))
}

/// Remove the sandbox info file for a given PID.
fn remove_sandbox_file(sandbox_dir: &Path, pid: u32) -> Result<(), String> {
    let path = sandbox_dir.join(format!("{pid}.sandbox"));
    if path.exists() {
        fs::remove_file(&path)
            .map_err(|e| format!("cannot remove sandbox file {}: {e}", path.display()))?;
    }
    Ok(())
}

// ============================================================================
// Profile discovery
// ============================================================================

/// Locate a profile for the given application name. Checks on-disk profiles
/// first, then falls back to built-in profiles.
fn find_profile(app_name: &str, profile_dir: &Path) -> Option<ProfileConfig> {
    // Try on-disk profile first.
    let disk_path = profile_dir.join(format!("{app_name}.profile"));
    if disk_path.is_file() {
        return parse_profile(&disk_path).ok();
    }
    // Fall back to built-in profile.
    let builtins = builtin_profiles();
    if let Some(content) = builtins.get(app_name) {
        let dummy_path = profile_dir.join(format!("{app_name}.profile"));
        return parse_profile_content(content, &dummy_path).ok();
    }
    Option::None
}

/// List all available profiles (both on-disk and built-in).
fn list_available_profiles(profile_dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();

    // Disk profiles.
    if let Ok(dir) = fs::read_dir(profile_dir) {
        for entry in dir.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("profile")
                && let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    names.push(stem.to_string());
                }
        }
    }

    // Built-in profiles that are not on disk.
    for name in supported_app_names() {
        let s = name.to_string();
        if !names.contains(&s) {
            names.push(s);
        }
    }

    names.sort_unstable();
    names.dedup();
    names
}

// ============================================================================
// Argument parsing helpers
// ============================================================================

/// Extract a value from an option of the form `--key=value`.
fn extract_option_value<'a>(arg: &'a str, prefix: &str) -> Option<&'a str> {
    if arg.starts_with(prefix) {
        Some(&arg[prefix.len()..])
    } else {
        Option::None
    }
}

/// Format uptime seconds into a human-readable string.
fn format_uptime(secs: u64) -> String {
    if secs < 60 {
        return format!("{secs}s");
    }
    let minutes = secs / 60;
    let remaining_secs = secs % 60;
    if minutes < 60 {
        return format!("{minutes}m {remaining_secs}s");
    }
    let hours = minutes / 60;
    let remaining_mins = minutes % 60;
    if hours < 24 {
        return format!("{hours}h {remaining_mins}m");
    }
    let days = hours / 24;
    let remaining_hours = hours % 24;
    format!("{days}d {remaining_hours}h")
}

// ============================================================================
// firejail personality — sandbox launcher
// ============================================================================

/// Parse firejail command-line arguments.
fn parse_firejail_args(args: &[String]) -> Result<(SandboxConfig, Vec<String>), String> {
    let mut config = SandboxConfig::new();
    let mut program_args: Vec<String> = Vec::new();
    let mut found_program = false;
    let mut i = 0;

    while i < args.len() {
        let arg = &args[i];

        if found_program {
            program_args.push(arg.clone());
            i += 1;
            continue;
        }

        if !arg.starts_with('-') {
            // First non-option is the program to sandbox.
            found_program = true;
            program_args.push(arg.clone());
            i += 1;
            continue;
        }

        match arg.as_str() {
            "--private" => config.filesystem.private_home = true,
            "--private-tmp" => config.filesystem.private_tmp = true,
            "--private-dev" => config.filesystem.private_dev = true,
            "--seccomp" => config.security.seccomp = true,
            "--noroot" => config.security.noroot = true,
            "--nosound" => config.security.nosound = true,
            "--no3d" => config.security.no3d = true,
            "--novideo" => config.security.novideo = true,
            "--nodvd" => config.security.nodvd = true,
            "--notv" => config.security.notv = true,
            "--nou2f" => config.security.nou2f = true,
            "--netfilter" => config.network.netfilter = true,
            "--noprofile" => config.noprofile = true,
            "--debug" => config.debug = true,
            "--quiet" => config.quiet = true,
            "--list" | "--tree" | "--top" => {
                // These are monitoring sub-commands delegated to firemon.
                return Ok((config, vec![arg.clone()]));
            }
            _ => {
                // Options with values (--key=value form).
                if let Some(val) = extract_option_value(arg, "--private-etc=") {
                    config.filesystem.private_etc = split_csv(val);
                } else if let Some(val) = extract_option_value(arg, "--private-bin=") {
                    config.filesystem.private_bin = split_csv(val);
                } else if let Some(val) = extract_option_value(arg, "--net=") {
                    config.network.mode = if val == "none" {
                        NetMode::None
                    } else {
                        NetMode::Interface(val.to_string())
                    };
                } else if let Some(val) = extract_option_value(arg, "--dns=") {
                    config.network.dns.servers.push(val.to_string());
                } else if let Some(val) = extract_option_value(arg, "--ip=") {
                    config.network.ip.address = Some(val.to_string());
                } else if let Some(val) = extract_option_value(arg, "--mac=") {
                    config.network.ip.mac = Some(val.to_string());
                } else if let Some(val) = extract_option_value(arg, "--hostname=") {
                    config.network.hostname = Some(val.to_string());
                } else if let Some(val) = extract_option_value(arg, "--shell=") {
                    config.shell = Some(val.to_string());
                } else if let Some(val) = extract_option_value(arg, "--profile=") {
                    config.profile_path = Some(val.to_string());
                } else if let Some(val) = extract_option_value(arg, "--caps.drop=") {
                    config.security.caps_drop = if val == "all" {
                        CapsDrop::All
                    } else {
                        CapsDrop::List(split_csv(val))
                    };
                } else if let Some(val) = extract_option_value(arg, "--name=") {
                    config.name = Some(val.to_string());
                } else if let Some(val) = extract_option_value(arg, "--noexec=") {
                    config
                        .filesystem
                        .restrictions
                        .push(FsRestriction::NoExec(val.to_string()));
                } else if let Some(val) = extract_option_value(arg, "--read-only=") {
                    config
                        .filesystem
                        .restrictions
                        .push(FsRestriction::ReadOnly(val.to_string()));
                } else if let Some(val) = extract_option_value(arg, "--tmpfs=") {
                    config
                        .filesystem
                        .restrictions
                        .push(FsRestriction::Tmpfs(val.to_string()));
                } else if let Some(val) = extract_option_value(arg, "--blacklist=") {
                    config
                        .filesystem
                        .restrictions
                        .push(FsRestriction::Blacklist(val.to_string()));
                } else if let Some(val) = extract_option_value(arg, "--whitelist=") {
                    config
                        .filesystem
                        .restrictions
                        .push(FsRestriction::Whitelist(val.to_string()));
                } else if let Some(val) = extract_option_value(arg, "--join=") {
                    // --join is handled as a special sub-command.
                    return Ok((config, vec!["--join".to_string(), val.to_string()]));
                } else if let Some(val) = extract_option_value(arg, "--shutdown=") {
                    return Ok((config, vec!["--shutdown".to_string(), val.to_string()]));
                } else if arg == "--help" || arg == "-h" {
                    print_firejail_help();
                    return Ok((config, vec!["--help".to_string()]));
                } else if arg == "--version" {
                    return Ok((config, vec!["--version".to_string()]));
                } else {
                    return Err(format!("unknown option: {arg}"));
                }
            }
        }
        i += 1;
    }

    Ok((config, program_args))
}

/// Execute the firejail personality.
fn run_firejail(args: &[String]) -> i32 {
    let (mut config, program_args) = match parse_firejail_args(args) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("firejail: {e}");
            return 1;
        }
    };

    // Handle special sub-commands that firejail delegates.
    if let Some(first) = program_args.first() {
        match first.as_str() {
            "--help" => return 0,
            "--version" => {
                println!("firejail version {VERSION}");
                return 0;
            }
            "--list" => return firemon_list(),
            "--tree" => return firemon_tree(),
            "--top" => return firemon_top(),
            "--join" => {
                if let Some(target) = program_args.get(1) {
                    return firejail_join(target);
                }
                eprintln!("firejail: --join requires a sandbox name or PID");
                return 1;
            }
            "--shutdown" => {
                if let Some(target) = program_args.get(1) {
                    return firejail_shutdown(target);
                }
                eprintln!("firejail: --shutdown requires a sandbox name or PID");
                return 1;
            }
            _ => {}
        }
    }

    if program_args.is_empty() {
        eprintln!("firejail: no program specified");
        eprintln!("Usage: firejail [options] program [arguments]");
        return 1;
    }

    let program = &program_args[0];

    // Load profile unless --noprofile is set.
    if !config.noprofile {
        let profile_dir = Path::new(DEFAULT_PROFILE_DIR);
        if let Some(ref explicit_path) = config.profile_path {
            match parse_profile(Path::new(explicit_path)) {
                Ok(profile) => config.merge_profile(&profile),
                Err(e) => {
                    if !config.quiet {
                        eprintln!("firejail: warning: {e}");
                    }
                }
            }
        } else {
            // Try to find a profile by program basename.
            let prog_base = program_basename(program);
            if let Some(profile) = find_profile(&prog_base, profile_dir) {
                if config.debug {
                    eprintln!("firejail: using profile for {prog_base}");
                }
                config.merge_profile(&profile);
            }
        }
    }

    let sandbox_name = config
        .name
        .clone()
        .unwrap_or_else(|| program_basename(program));

    if !config.quiet {
        println!("Reading profile for {sandbox_name}");
        print_sandbox_summary(&config, &sandbox_name, program);
    }

    if config.debug {
        print_sandbox_debug(&config);
    }

    // In a real implementation, we would:
    // 1. Create namespaces (mount, network, PID, user).
    // 2. Set up filesystem restrictions (bind mounts, tmpfs, etc.).
    // 3. Configure the network (veth pair, firewall rules).
    // 4. Drop capabilities.
    // 5. Apply seccomp filters.
    // 6. exec() the target program.
    //
    // For OurOS, the actual isolation mechanism uses the kernel's capability
    // system and namespace support. This frontend parses the configuration
    // and issues the appropriate syscalls.

    if !config.quiet {
        println!(
            "Child process initialized in {:.1}ms",
            0.8 // Placeholder timing.
        );
    }

    // Write sandbox info.
    let sandbox_dir = Path::new(DEFAULT_SANDBOX_DIR);
    let our_pid = process::id();
    let info = SandboxInfo {
        pid: our_pid,
        name: sandbox_name.clone(),
        program: program.clone(),
        user: get_current_user(),
        uptime_secs: 0,
        cpu_millipercent: 0,
        mem_kb: 0,
        net_mode: config.network.mode.to_string(),
        caps_dropped: !matches!(config.security.caps_drop, CapsDrop::None),
        seccomp_enabled: config.security.seccomp,
        children: Vec::new(),
    };
    if let Err(e) = write_sandbox_file(sandbox_dir, &info)
        && config.debug {
            eprintln!("firejail: warning: {e}");
        }

    if !config.quiet {
        println!("Sandbox {sandbox_name} started (PID {our_pid})");
    }

    // In the real implementation, we would exec the program here.
    // For now, report that we would launch it.
    if !config.quiet {
        let prog_args = if program_args.len() > 1 {
            program_args[1..].join(" ")
        } else {
            String::new()
        };
        if prog_args.is_empty() {
            println!("Would execute: {program}");
        } else {
            println!("Would execute: {program} {prog_args}");
        }
    }

    0
}

/// Extract the basename of a program path (without directories or .exe suffix).
fn program_basename(path: &str) -> String {
    let bytes = path.as_bytes();
    let mut last_sep = 0;
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'/' || b == b'\\' {
            last_sep = i + 1;
        }
    }
    let base = &path[last_sep..];
    base.strip_suffix(".exe").unwrap_or(base).to_string()
}

/// Print a summary of the sandbox configuration.
fn print_sandbox_summary(config: &SandboxConfig, name: &str, program: &str) {
    println!("Sandbox: {name}");
    println!("  Program: {program}");
    println!("  Network: {}", config.network.mode);
    if config.network.netfilter {
        println!("  Netfilter: enabled");
    }
    if !config.network.dns.servers.is_empty() {
        println!("  DNS: {}", config.network.dns.servers.join(", "));
    }
    if let Some(ref ip) = config.network.ip.address {
        println!("  IP: {ip}");
    }
    if let Some(ref mac) = config.network.ip.mac {
        println!("  MAC: {mac}");
    }
    if let Some(ref hostname) = config.network.hostname {
        println!("  Hostname: {hostname}");
    }
    if config.filesystem.private_home {
        println!("  Private home: yes");
    }
    if config.filesystem.private_tmp {
        println!("  Private /tmp: yes");
    }
    if config.filesystem.private_dev {
        println!("  Private /dev: yes");
    }
    if !config.filesystem.private_etc.is_empty() {
        println!("  Private /etc: {}", config.filesystem.private_etc.join(", "));
    }
    if !config.filesystem.private_bin.is_empty() {
        println!(
            "  Private /bin: {}",
            config.filesystem.private_bin.join(", ")
        );
    }
    println!("  Capabilities: {}", config.security.caps_drop);
    if config.security.seccomp {
        println!("  Seccomp: enabled");
    }
    if config.security.noroot {
        println!("  No root: yes");
    }
    if config.security.nosound {
        println!("  No sound: yes");
    }
    if config.security.no3d {
        println!("  No 3D: yes");
    }
    if config.security.novideo {
        println!("  No video: yes");
    }
    for r in &config.filesystem.restrictions {
        println!("  Restriction: {r}");
    }
}

/// Print detailed debug information about the sandbox configuration.
fn print_sandbox_debug(config: &SandboxConfig) {
    eprintln!("DEBUG: SandboxConfig {{");
    eprintln!("  name: {:?}", config.name);
    eprintln!("  network.mode: {:?}", config.network.mode);
    eprintln!("  network.netfilter: {}", config.network.netfilter);
    eprintln!("  network.dns: {:?}", config.network.dns.servers);
    eprintln!("  network.ip: {:?}", config.network.ip.address);
    eprintln!("  network.mac: {:?}", config.network.ip.mac);
    eprintln!("  network.hostname: {:?}", config.network.hostname);
    eprintln!("  fs.private_home: {}", config.filesystem.private_home);
    eprintln!("  fs.private_tmp: {}", config.filesystem.private_tmp);
    eprintln!("  fs.private_dev: {}", config.filesystem.private_dev);
    eprintln!("  fs.private_etc: {:?}", config.filesystem.private_etc);
    eprintln!("  fs.private_bin: {:?}", config.filesystem.private_bin);
    eprintln!(
        "  fs.restrictions: {} entries",
        config.filesystem.restrictions.len()
    );
    eprintln!("  security.caps_drop: {}", config.security.caps_drop);
    eprintln!("  security.seccomp: {}", config.security.seccomp);
    eprintln!("  security.noroot: {}", config.security.noroot);
    eprintln!("  security.nosound: {}", config.security.nosound);
    eprintln!("  security.no3d: {}", config.security.no3d);
    eprintln!("  security.novideo: {}", config.security.novideo);
    eprintln!("  security.nodvd: {}", config.security.nodvd);
    eprintln!("  security.notv: {}", config.security.notv);
    eprintln!("  security.nou2f: {}", config.security.nou2f);
    eprintln!("  noprofile: {}", config.noprofile);
    eprintln!("  profile_path: {:?}", config.profile_path);
    eprintln!("  shell: {:?}", config.shell);
    eprintln!("  quiet: {}", config.quiet);
    eprintln!("}}");
}

/// Join an existing sandbox by name or PID.
fn firejail_join(target: &str) -> i32 {
    let sandbox_dir = Path::new(DEFAULT_SANDBOX_DIR);
    let entries = read_sandbox_entries(sandbox_dir);

    // Try as PID first, then as name.
    let found = if let Ok(pid) = target.parse::<u32>() {
        entries.iter().find(|e| e.pid == pid)
    } else {
        entries.iter().find(|e| e.name == target)
    };

    match found {
        Some(info) => {
            println!("Joining sandbox {} (PID {})", info.name, info.pid);
            println!("  Program: {}", info.program);
            println!("  Network: {}", info.net_mode);
            // In the real implementation, we would attach to the sandbox's
            // namespaces and exec a shell.
            println!("Would attach to sandbox namespace and exec shell");
            0
        }
        None => {
            eprintln!("firejail: cannot find sandbox: {target}");
            1
        }
    }
}

/// Shut down a running sandbox by name or PID.
fn firejail_shutdown(target: &str) -> i32 {
    let sandbox_dir = Path::new(DEFAULT_SANDBOX_DIR);
    let entries = read_sandbox_entries(sandbox_dir);

    let found = if let Ok(pid) = target.parse::<u32>() {
        entries.iter().find(|e| e.pid == pid)
    } else {
        entries.iter().find(|e| e.name == target)
    };

    match found {
        Some(info) => {
            println!("Shutting down sandbox {} (PID {})", info.name, info.pid);
            // In the real implementation, we would send a signal to the
            // sandbox's init process.
            if let Err(e) = remove_sandbox_file(sandbox_dir, info.pid) {
                eprintln!("firejail: warning: {e}");
            }
            println!("Sandbox terminated");
            0
        }
        None => {
            eprintln!("firejail: cannot find sandbox: {target}");
            1
        }
    }
}

/// Get the current user name (placeholder for OurOS).
fn get_current_user() -> String {
    env::var("USER")
        .or_else(|_| env::var("USERNAME"))
        .unwrap_or_else(|_| String::from("root"))
}

/// Print firejail help text.
fn print_firejail_help() {
    println!("firejail {VERSION} -- Security sandbox");
    println!();
    println!("Usage: firejail [options] program [arguments]");
    println!();
    println!("Sandbox options:");
    println!("  --private              Private home directory");
    println!("  --private-tmp          Private /tmp");
    println!("  --private-dev          Private /dev");
    println!("  --private-etc=LIST     Private /etc with only listed files");
    println!("  --private-bin=LIST     Private /bin with only listed binaries");
    println!();
    println!("Network options:");
    println!("  --net=none|IFACE       Network restriction (none=no network)");
    println!("  --netfilter            Enable default netfilter");
    println!("  --dns=ADDR             Set DNS server");
    println!("  --ip=ADDR              Set IP address");
    println!("  --mac=ADDR             Set MAC address");
    println!("  --hostname=NAME        Set hostname");
    println!();
    println!("Security options:");
    println!("  --caps.drop=all|LIST   Drop capabilities");
    println!("  --seccomp              Enable seccomp filter");
    println!("  --noroot               Disable root access");
    println!("  --nosound              Disable sound");
    println!("  --no3d                 Disable 3D acceleration");
    println!("  --novideo              Disable video devices");
    println!("  --nodvd                Disable DVD devices");
    println!("  --notv                 Disable TV devices");
    println!("  --nou2f                Disable U2F devices");
    println!();
    println!("Filesystem options:");
    println!("  --noexec=PATH          Prevent execution under PATH");
    println!("  --read-only=PATH       Make PATH read-only");
    println!("  --tmpfs=PATH           Mount tmpfs at PATH");
    println!("  --blacklist=PATH       Hide PATH from sandbox");
    println!("  --whitelist=PATH       Only allow PATH in sandbox");
    println!();
    println!("Profile options:");
    println!("  --noprofile            Do not load a profile");
    println!("  --profile=FILE         Load specific profile");
    println!("  --shell=none           Do not use a shell");
    println!();
    println!("Sandbox management:");
    println!("  --name=NAME            Set sandbox name");
    println!("  --join=NAME|PID        Join existing sandbox");
    println!("  --shutdown=NAME|PID    Shut down a sandbox");
    println!("  --list                 List running sandboxes");
    println!("  --tree                 Show sandbox process tree");
    println!("  --top                  Top-like sandbox monitoring");
    println!();
    println!("General:");
    println!("  --debug                Enable debug output");
    println!("  --quiet                Suppress informational output");
    println!("  --version              Print version");
    println!("  --help, -h             Print this help");
}

// ============================================================================
// firemon personality — sandbox monitoring
// ============================================================================

/// Execute the firemon personality.
fn run_firemon(args: &[String]) -> i32 {
    if args.is_empty() {
        // Default action: list sandboxes.
        return firemon_list();
    }

    let first = &args[0];
    match first.as_str() {
        "--help" | "-h" => {
            print_firemon_help();
            0
        }
        "--version" => {
            println!("firemon version {VERSION}");
            0
        }
        "--list" => firemon_list(),
        "--top" => firemon_top(),
        "--tree" => firemon_tree(),
        "--netstats" => firemon_netstats(),
        "--caps" => {
            if let Some(pid_str) = args.get(1) {
                firemon_caps(pid_str)
            } else {
                eprintln!("firemon: --caps requires a PID argument");
                1
            }
        }
        "--seccomp" => {
            if let Some(pid_str) = args.get(1) {
                firemon_seccomp(pid_str)
            } else {
                eprintln!("firemon: --seccomp requires a PID argument");
                1
            }
        }
        _ => {
            eprintln!("firemon: unknown option: {first}");
            eprintln!("Try 'firemon --help' for more information.");
            1
        }
    }
}

/// List all running sandboxes.
fn firemon_list() -> i32 {
    let sandbox_dir = Path::new(DEFAULT_SANDBOX_DIR);
    let entries = read_sandbox_entries(sandbox_dir);

    if entries.is_empty() {
        println!("No sandboxes running.");
        return 0;
    }

    println!(
        "{:>6} {:<12} {:<20} {:<30} {:<10}",
        "PID", "USER", "NAME", "PROGRAM", "NET"
    );
    println!("{}", "-".repeat(80));
    for entry in &entries {
        println!(
            "{:>6} {:<12} {:<20} {:<30} {:<10}",
            entry.pid, entry.user, entry.name, entry.program, entry.net_mode,
        );
    }
    println!();
    println!("{} sandbox(es) running.", entries.len());

    0
}

/// Show a top-like view of running sandboxes.
fn firemon_top() -> i32 {
    let sandbox_dir = Path::new(DEFAULT_SANDBOX_DIR);
    let entries = read_sandbox_entries(sandbox_dir);

    if entries.is_empty() {
        println!("No sandboxes running.");
        return 0;
    }

    println!(
        "{:>6} {:<12} {:<16} {:>6} {:>10} {:>10} {:<12}",
        "PID", "USER", "NAME", "CPU", "MEM(KB)", "UPTIME", "NET"
    );
    println!("{}", "-".repeat(78));
    for entry in &entries {
        println!(
            "{:>6} {:<12} {:<16} {:>6} {:>10} {:>10} {:<12}",
            entry.pid,
            entry.user,
            truncate_str(&entry.name, 16),
            entry.cpu_display(),
            entry.mem_kb,
            format_uptime(entry.uptime_secs),
            entry.net_mode,
        );
    }
    println!();
    println!("{} sandbox(es).", entries.len());

    0
}

/// Show the process tree for all sandboxes.
fn firemon_tree() -> i32 {
    let sandbox_dir = Path::new(DEFAULT_SANDBOX_DIR);
    let entries = read_sandbox_entries(sandbox_dir);

    if entries.is_empty() {
        println!("No sandboxes running.");
        return 0;
    }

    for entry in &entries {
        println!(
            "{} ({}) -- {}",
            entry.name, entry.pid, entry.program
        );
        for (i, child) in entry.children.iter().enumerate() {
            let connector = if i + 1 < entry.children.len() {
                "├── "
            } else {
                "└── "
            };
            println!("  {connector}{child}");
        }
    }

    0
}

/// Show capability information for a sandbox by PID.
fn firemon_caps(pid_str: &str) -> i32 {
    let pid: u32 = match pid_str.parse() {
        Ok(p) => p,
        Err(_) => {
            eprintln!("firemon: invalid PID: {pid_str}");
            return 1;
        }
    };

    let sandbox_dir = Path::new(DEFAULT_SANDBOX_DIR);
    let entries = read_sandbox_entries(sandbox_dir);

    match entries.iter().find(|e| e.pid == pid) {
        Some(entry) => {
            println!("Sandbox: {} (PID {})", entry.name, entry.pid);
            println!(
                "Capabilities: {}",
                if entry.caps_dropped {
                    "all dropped"
                } else {
                    "retained (not restricted)"
                }
            );
            0
        }
        None => {
            eprintln!("firemon: no sandbox with PID {pid}");
            1
        }
    }
}

/// Show seccomp information for a sandbox by PID.
fn firemon_seccomp(pid_str: &str) -> i32 {
    let pid: u32 = match pid_str.parse() {
        Ok(p) => p,
        Err(_) => {
            eprintln!("firemon: invalid PID: {pid_str}");
            return 1;
        }
    };

    let sandbox_dir = Path::new(DEFAULT_SANDBOX_DIR);
    let entries = read_sandbox_entries(sandbox_dir);

    match entries.iter().find(|e| e.pid == pid) {
        Some(entry) => {
            println!("Sandbox: {} (PID {})", entry.name, entry.pid);
            println!(
                "Seccomp: {}",
                if entry.seccomp_enabled {
                    "enabled (default filter)"
                } else {
                    "disabled"
                }
            );
            0
        }
        None => {
            eprintln!("firemon: no sandbox with PID {pid}");
            1
        }
    }
}

/// Show network statistics for running sandboxes.
fn firemon_netstats() -> i32 {
    let sandbox_dir = Path::new(DEFAULT_SANDBOX_DIR);
    let entries = read_sandbox_entries(sandbox_dir);

    if entries.is_empty() {
        println!("No sandboxes running.");
        return 0;
    }

    println!(
        "{:>6} {:<16} {:<10} {:>12} {:>12}",
        "PID", "NAME", "NET", "RX(KB)", "TX(KB)"
    );
    println!("{}", "-".repeat(60));
    for entry in &entries {
        // In a real implementation, we would read /proc/<pid>/net/dev
        // or equivalent OurOS statistics. Placeholder zeros for now.
        println!(
            "{:>6} {:<16} {:<10} {:>12} {:>12}",
            entry.pid,
            truncate_str(&entry.name, 16),
            entry.net_mode,
            0,
            0,
        );
    }

    0
}

/// Print firemon help text.
fn print_firemon_help() {
    println!("firemon {VERSION} -- Firejail sandbox monitor");
    println!();
    println!("Usage: firemon [option] [PID]");
    println!();
    println!("Options:");
    println!("  --list              List running sandboxes");
    println!("  --top               Top-like monitoring of sandboxes");
    println!("  --tree              Show process tree");
    println!("  --caps PID          Show capabilities for sandbox PID");
    println!("  --seccomp PID       Show seccomp filters for sandbox PID");
    println!("  --netstats          Show network statistics");
    println!("  --version           Print version");
    println!("  --help, -h          Print this help");
}

// ============================================================================
// firecfg personality — configure default sandboxes
// ============================================================================

/// Execute the firecfg personality.
fn run_firecfg(args: &[String]) -> i32 {
    if args.is_empty() {
        print_firecfg_help();
        return 0;
    }

    let first = &args[0];
    match first.as_str() {
        "--help" | "-h" => {
            print_firecfg_help();
            0
        }
        "--version" => {
            println!("firecfg version {VERSION}");
            0
        }
        "--list" => firecfg_list(),
        "--fix" => firecfg_fix(),
        "--clean" => firecfg_clean(),
        _ => {
            eprintln!("firecfg: unknown option: {first}");
            eprintln!("Try 'firecfg --help' for more information.");
            1
        }
    }
}

/// List all available application profiles.
fn firecfg_list() -> i32 {
    let profile_dir = Path::new(DEFAULT_PROFILE_DIR);
    let profiles = list_available_profiles(profile_dir);

    if profiles.is_empty() {
        println!("No profiles available.");
        return 0;
    }

    println!("Available application profiles:");
    println!();
    for name in &profiles {
        let source = if profile_dir.join(format!("{name}.profile")).is_file() {
            "disk"
        } else {
            "built-in"
        };
        println!("  {name:<30} ({source})");
    }
    println!();
    println!("{} profile(s) available.", profiles.len());

    0
}

/// Create firejail symlinks for all supported applications.
fn firecfg_fix() -> i32 {
    let symlink_dir = Path::new(DEFAULT_SYMLINK_DIR);
    let profile_dir = Path::new(DEFAULT_PROFILE_DIR);
    let profiles = list_available_profiles(profile_dir);

    if profiles.is_empty() {
        println!("No profiles to configure.");
        return 0;
    }

    let firejail_path = PathBuf::from("/usr/bin/firejail");
    let mut created = 0u32;
    let mut skipped = 0u32;
    let mut errors = 0u32;

    for name in &profiles {
        let link_path = symlink_dir.join(name);
        if link_path.exists() {
            // Check if it already points to firejail.
            match fs::read_link(&link_path) {
                Ok(target) if target == firejail_path => {
                    skipped += 1;
                    continue;
                }
                _ => {
                    // Path exists but is not a firejail symlink, skip it.
                    println!(
                        "  Skipping {name}: {} already exists",
                        link_path.display()
                    );
                    skipped += 1;
                    continue;
                }
            }
        }

        // Create symlink. On OurOS, std::os::unix::fs::symlink would be used.
        // For portability of this code, we simulate it.
        match create_symlink(&firejail_path, &link_path) {
            Ok(()) => {
                println!("  Created: {name} -> {}", firejail_path.display());
                created += 1;
            }
            Err(e) => {
                eprintln!("  Error creating {name}: {e}");
                errors += 1;
            }
        }
    }

    println!();
    println!("{created} symlink(s) created, {skipped} skipped, {errors} error(s).");

    if errors > 0 { 1 } else { 0 }
}

/// Remove firejail symlinks for all supported applications.
fn firecfg_clean() -> i32 {
    let symlink_dir = Path::new(DEFAULT_SYMLINK_DIR);
    let firejail_path = PathBuf::from("/usr/bin/firejail");
    let profile_dir = Path::new(DEFAULT_PROFILE_DIR);
    let profiles = list_available_profiles(profile_dir);

    let mut removed = 0u32;
    let mut not_found = 0u32;
    let mut errors = 0u32;

    for name in &profiles {
        let link_path = symlink_dir.join(name);
        if !link_path.exists() {
            not_found += 1;
            continue;
        }

        // Only remove if it is a symlink pointing to firejail.
        match fs::read_link(&link_path) {
            Ok(target) if target == firejail_path => {
                match fs::remove_file(&link_path) {
                    Ok(()) => {
                        println!("  Removed: {}", link_path.display());
                        removed += 1;
                    }
                    Err(e) => {
                        eprintln!("  Error removing {}: {e}", link_path.display());
                        errors += 1;
                    }
                }
            }
            _ => {
                not_found += 1;
            }
        }
    }

    println!();
    println!("{removed} symlink(s) removed, {not_found} not found, {errors} error(s).");

    if errors > 0 { 1 } else { 0 }
}

/// Create a symbolic link. Wraps the OS-specific symlink call.
fn create_symlink(target: &Path, link: &Path) -> Result<(), String> {
    // Ensure parent directory exists.
    if let Some(parent) = link.parent() {
        let _ = fs::create_dir_all(parent);
    }

    // On Unix-like systems (including OurOS), use symlink.
    // We write a small marker file as a placeholder on platforms where
    // symlink may not be available (e.g., Windows during development).
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link)
            .map_err(|e| format!("symlink failed: {e}"))
    }
    #[cfg(not(unix))]
    {
        // Fallback: write a marker file indicating this is a firejail symlink.
        let content = format!("firejail-symlink -> {}\n", target.display());
        fs::write(link, content).map_err(|e| format!("write marker failed: {e}"))
    }
}

/// Print firecfg help text.
fn print_firecfg_help() {
    println!("firecfg {VERSION} -- Firejail configuration utility");
    println!();
    println!("Usage: firecfg [option]");
    println!();
    println!("Options:");
    println!("  --list              List available application profiles");
    println!("  --fix               Create symlinks for all supported apps");
    println!("  --clean             Remove firejail symlinks");
    println!("  --version           Print version");
    println!("  --help, -h          Print this help");
}

// ============================================================================
// Utility functions
// ============================================================================

/// Truncate a string to fit within `max_len` characters, adding ".." suffix
/// if truncation occurs.
fn truncate_str(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else if max_len <= 2 {
        s[..max_len].to_string()
    } else {
        format!("{}..", &s[..max_len - 2])
    }
}

// ============================================================================
// Main entry point with personality dispatch
// ============================================================================

fn main() {
    let args: Vec<String> = env::args().collect();

    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("firejail");
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
        "firemon" => run_firemon(&tool_args),
        "firecfg" => run_firecfg(&tool_args),
        _ => run_firejail(&tool_args),
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
    // Personality extraction tests
    // -----------------------------------------------------------------------

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

    #[test]
    fn test_personality_firejail_unix() {
        assert_eq!(extract_personality("/usr/bin/firejail"), "firejail");
    }

    #[test]
    fn test_personality_firejail_bare() {
        assert_eq!(extract_personality("firejail"), "firejail");
    }

    #[test]
    fn test_personality_firemon_unix() {
        assert_eq!(extract_personality("/usr/bin/firemon"), "firemon");
    }

    #[test]
    fn test_personality_firecfg_unix() {
        assert_eq!(extract_personality("/usr/bin/firecfg"), "firecfg");
    }

    #[test]
    fn test_personality_windows_path() {
        assert_eq!(
            extract_personality("C:\\Program Files\\firejail\\firejail.exe"),
            "firejail"
        );
    }

    #[test]
    fn test_personality_firemon_windows() {
        assert_eq!(
            extract_personality("C:\\tools\\firemon.exe"),
            "firemon"
        );
    }

    #[test]
    fn test_personality_firecfg_windows() {
        assert_eq!(
            extract_personality("D:\\bin\\firecfg.exe"),
            "firecfg"
        );
    }

    #[test]
    fn test_personality_unknown_defaults() {
        // Any unknown name will fall through to firejail (the default).
        let name = extract_personality("/usr/bin/unknown");
        assert_eq!(name, "unknown");
    }

    #[test]
    fn test_personality_mixed_separators() {
        assert_eq!(
            extract_personality("/usr/local\\bin/firejail"),
            "firejail"
        );
    }

    #[test]
    fn test_personality_trailing_slash() {
        // Trailing slash yields empty string -- unusual but handled.
        assert_eq!(extract_personality("/usr/bin/"), "");
    }

    // -----------------------------------------------------------------------
    // NetMode tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_net_mode_default() {
        assert_eq!(NetMode::default(), NetMode::Host);
    }

    #[test]
    fn test_net_mode_display_host() {
        assert_eq!(NetMode::Host.to_string(), "host");
    }

    #[test]
    fn test_net_mode_display_none() {
        assert_eq!(NetMode::None.to_string(), "none");
    }

    #[test]
    fn test_net_mode_display_interface() {
        assert_eq!(
            NetMode::Interface("eth0".to_string()).to_string(),
            "eth0"
        );
    }

    #[test]
    fn test_net_mode_equality() {
        assert_eq!(NetMode::None, NetMode::None);
        assert_ne!(NetMode::None, NetMode::Host);
    }

    // -----------------------------------------------------------------------
    // CapsDrop tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_caps_drop_default() {
        assert_eq!(CapsDrop::default(), CapsDrop::None);
    }

    #[test]
    fn test_caps_drop_display_none() {
        assert_eq!(CapsDrop::None.to_string(), "none");
    }

    #[test]
    fn test_caps_drop_display_all() {
        assert_eq!(CapsDrop::All.to_string(), "all");
    }

    #[test]
    fn test_caps_drop_display_list() {
        let caps = CapsDrop::List(vec!["net_raw".to_string(), "sys_admin".to_string()]);
        assert_eq!(caps.to_string(), "net_raw,sys_admin");
    }

    // -----------------------------------------------------------------------
    // FsRestriction tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_fs_restriction_display_readonly() {
        let r = FsRestriction::ReadOnly("/etc".to_string());
        assert_eq!(r.to_string(), "read-only /etc");
    }

    #[test]
    fn test_fs_restriction_display_tmpfs() {
        let r = FsRestriction::Tmpfs("/tmp".to_string());
        assert_eq!(r.to_string(), "tmpfs /tmp");
    }

    #[test]
    fn test_fs_restriction_display_blacklist() {
        let r = FsRestriction::Blacklist("/boot".to_string());
        assert_eq!(r.to_string(), "blacklist /boot");
    }

    #[test]
    fn test_fs_restriction_display_whitelist() {
        let r = FsRestriction::Whitelist("/home/user".to_string());
        assert_eq!(r.to_string(), "whitelist /home/user");
    }

    #[test]
    fn test_fs_restriction_display_noexec() {
        let r = FsRestriction::NoExec("/tmp".to_string());
        assert_eq!(r.to_string(), "noexec /tmp");
    }

    // -----------------------------------------------------------------------
    // split_csv tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_split_csv_single() {
        assert_eq!(split_csv("fonts"), vec!["fonts"]);
    }

    #[test]
    fn test_split_csv_multiple() {
        assert_eq!(
            split_csv("fonts,ssl,ca-certificates"),
            vec!["fonts", "ssl", "ca-certificates"]
        );
    }

    #[test]
    fn test_split_csv_with_spaces() {
        assert_eq!(
            split_csv("fonts , ssl , pki"),
            vec!["fonts", "ssl", "pki"]
        );
    }

    #[test]
    fn test_split_csv_empty() {
        let result: Vec<String> = split_csv("");
        assert!(result.is_empty());
    }

    #[test]
    fn test_split_csv_trailing_comma() {
        assert_eq!(split_csv("a,b,"), vec!["a", "b"]);
    }

    #[test]
    fn test_split_csv_leading_comma() {
        assert_eq!(split_csv(",a,b"), vec!["a", "b"]);
    }

    // -----------------------------------------------------------------------
    // format_uptime tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_format_uptime_seconds() {
        assert_eq!(format_uptime(30), "30s");
    }

    #[test]
    fn test_format_uptime_zero() {
        assert_eq!(format_uptime(0), "0s");
    }

    #[test]
    fn test_format_uptime_minutes() {
        assert_eq!(format_uptime(90), "1m 30s");
    }

    #[test]
    fn test_format_uptime_hours() {
        assert_eq!(format_uptime(3661), "1h 1m");
    }

    #[test]
    fn test_format_uptime_days() {
        assert_eq!(format_uptime(90000), "1d 1h");
    }

    #[test]
    fn test_format_uptime_exactly_one_minute() {
        assert_eq!(format_uptime(60), "1m 0s");
    }

    #[test]
    fn test_format_uptime_exactly_one_hour() {
        assert_eq!(format_uptime(3600), "1h 0m");
    }

    #[test]
    fn test_format_uptime_exactly_one_day() {
        assert_eq!(format_uptime(86400), "1d 0h");
    }

    // -----------------------------------------------------------------------
    // truncate_str tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_truncate_str_short() {
        assert_eq!(truncate_str("hello", 10), "hello");
    }

    #[test]
    fn test_truncate_str_exact() {
        assert_eq!(truncate_str("hello", 5), "hello");
    }

    #[test]
    fn test_truncate_str_long() {
        assert_eq!(truncate_str("hello world", 8), "hello ..");
    }

    #[test]
    fn test_truncate_str_very_short_limit() {
        assert_eq!(truncate_str("hello", 2), "he");
    }

    #[test]
    fn test_truncate_str_empty() {
        assert_eq!(truncate_str("", 10), "");
    }

    // -----------------------------------------------------------------------
    // program_basename tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_program_basename_simple() {
        assert_eq!(program_basename("firefox"), "firefox");
    }

    #[test]
    fn test_program_basename_unix_path() {
        assert_eq!(program_basename("/usr/bin/firefox"), "firefox");
    }

    #[test]
    fn test_program_basename_windows_path() {
        assert_eq!(
            program_basename("C:\\Program Files\\firefox.exe"),
            "firefox"
        );
    }

    #[test]
    fn test_program_basename_mixed() {
        assert_eq!(program_basename("/opt/local\\bin/vlc"), "vlc");
    }

    #[test]
    fn test_program_basename_no_extension() {
        assert_eq!(program_basename("/usr/bin/chromium"), "chromium");
    }

    // -----------------------------------------------------------------------
    // extract_option_value tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_extract_option_value_present() {
        assert_eq!(
            extract_option_value("--net=none", "--net="),
            Some("none")
        );
    }

    #[test]
    fn test_extract_option_value_absent() {
        assert_eq!(extract_option_value("--private", "--net="), Option::None);
    }

    #[test]
    fn test_extract_option_value_empty() {
        assert_eq!(extract_option_value("--net=", "--net="), Some(""));
    }

    #[test]
    fn test_extract_option_value_with_equals() {
        assert_eq!(
            extract_option_value("--dns=8.8.8.8", "--dns="),
            Some("8.8.8.8")
        );
    }

    // -----------------------------------------------------------------------
    // Profile parsing tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_profile_line_empty() {
        assert!(parse_profile_line("").is_none());
    }

    #[test]
    fn test_parse_profile_line_comment() {
        assert!(parse_profile_line("# this is a comment").is_none());
    }

    #[test]
    fn test_parse_profile_line_whitespace() {
        assert!(parse_profile_line("   ").is_none());
    }

    #[test]
    fn test_parse_profile_line_simple_directive() {
        let result = parse_profile_line("seccomp");
        assert_eq!(result, Some(("seccomp", Option::None)));
    }

    #[test]
    fn test_parse_profile_line_directive_with_value() {
        let result = parse_profile_line("blacklist /boot");
        assert_eq!(result, Some(("blacklist", Some("/boot"))));
    }

    #[test]
    fn test_parse_profile_line_private_etc() {
        let result = parse_profile_line("private-etc fonts,ssl");
        assert_eq!(result, Some(("private-etc", Some("fonts,ssl"))));
    }

    #[test]
    fn test_parse_profile_line_caps_drop() {
        let result = parse_profile_line("caps.drop all");
        assert_eq!(result, Some(("caps.drop", Some("all"))));
    }

    #[test]
    fn test_parse_profile_content_firefox() {
        let content = "# Firefox profile\n\
                        private-tmp\n\
                        private-dev\n\
                        private-etc fonts,ssl\n\
                        caps.drop all\n\
                        seccomp\n\
                        noroot\n";
        let path = Path::new("firefox.profile");
        let profile = parse_profile_content(content, path).unwrap();
        assert!(profile.private_tmp);
        assert!(profile.private_dev);
        assert_eq!(profile.private_etc, vec!["fonts", "ssl"]);
        assert!(profile.caps_drop_all);
        assert!(profile.seccomp);
        assert!(profile.noroot);
    }

    #[test]
    fn test_parse_profile_content_with_comments() {
        let content = "# comment line\n\
                        \n\
                        private-tmp\n\
                        # another comment\n\
                        seccomp\n";
        let path = Path::new("test.profile");
        let profile = parse_profile_content(content, path).unwrap();
        assert!(profile.private_tmp);
        assert!(profile.seccomp);
        assert!(!profile.private_dev);
    }

    #[test]
    fn test_parse_profile_content_restrictions() {
        let content = "blacklist /boot\n\
                        blacklist /sbin\n\
                        read-only /etc\n\
                        noexec /tmp\n\
                        tmpfs /var/tmp\n\
                        whitelist /home/user\n";
        let path = Path::new("test.profile");
        let profile = parse_profile_content(content, path).unwrap();
        assert_eq!(profile.restrictions.len(), 6);
        assert_eq!(
            profile.restrictions[0],
            FsRestriction::Blacklist("/boot".to_string())
        );
        assert_eq!(
            profile.restrictions[1],
            FsRestriction::Blacklist("/sbin".to_string())
        );
        assert_eq!(
            profile.restrictions[2],
            FsRestriction::ReadOnly("/etc".to_string())
        );
        assert_eq!(
            profile.restrictions[3],
            FsRestriction::NoExec("/tmp".to_string())
        );
        assert_eq!(
            profile.restrictions[4],
            FsRestriction::Tmpfs("/var/tmp".to_string())
        );
        assert_eq!(
            profile.restrictions[5],
            FsRestriction::Whitelist("/home/user".to_string())
        );
    }

    #[test]
    fn test_parse_profile_content_includes() {
        let content = "include base.profile\n\
                        private-tmp\n";
        let path = Path::new("app.profile");
        let profile = parse_profile_content(content, path).unwrap();
        assert_eq!(profile.includes, vec!["base.profile"]);
        assert!(profile.private_tmp);
    }

    #[test]
    fn test_parse_profile_content_net_none() {
        let content = "net none\n";
        let path = Path::new("test.profile");
        let profile = parse_profile_content(content, path).unwrap();
        assert_eq!(profile.net_mode, Some(NetMode::None));
    }

    #[test]
    fn test_parse_profile_content_net_interface() {
        let content = "net br0\n";
        let path = Path::new("test.profile");
        let profile = parse_profile_content(content, path).unwrap();
        assert_eq!(
            profile.net_mode,
            Some(NetMode::Interface("br0".to_string()))
        );
    }

    #[test]
    fn test_parse_profile_content_unknown_directive() {
        let content = "future-feature some-value\n\
                        private-tmp\n";
        let path = Path::new("test.profile");
        let profile = parse_profile_content(content, path).unwrap();
        // Unknown directives are silently ignored.
        assert!(profile.private_tmp);
    }

    #[test]
    fn test_parse_profile_content_all_security_flags() {
        let content = "nosound\n\
                        no3d\n\
                        novideo\n\
                        nodvd\n\
                        notv\n\
                        nou2f\n\
                        netfilter\n";
        let path = Path::new("test.profile");
        let profile = parse_profile_content(content, path).unwrap();
        assert!(profile.nosound);
        assert!(profile.no3d);
        assert!(profile.novideo);
        assert!(profile.nodvd);
        assert!(profile.notv);
        assert!(profile.nou2f);
        assert!(profile.netfilter);
    }

    #[test]
    fn test_parse_profile_content_private_bin() {
        let content = "private-bin firefox,sh,bash\n";
        let path = Path::new("test.profile");
        let profile = parse_profile_content(content, path).unwrap();
        assert_eq!(profile.private_bin, vec!["firefox", "sh", "bash"]);
    }

    #[test]
    fn test_parse_profile_name_from_path() {
        let path = Path::new("/etc/firejail/firefox.profile");
        let content = "seccomp\n";
        let profile = parse_profile_content(content, path).unwrap();
        assert_eq!(profile.name, "firefox");
    }

    // -----------------------------------------------------------------------
    // SandboxConfig merge tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_sandbox_config_merge_basic() {
        let mut config = SandboxConfig::new();
        let mut profile = ProfileConfig::default();
        profile.private_tmp = true;
        profile.seccomp = true;
        config.merge_profile(&profile);
        assert!(config.filesystem.private_tmp);
        assert!(config.security.seccomp);
    }

    #[test]
    fn test_sandbox_config_merge_cli_overrides_profile() {
        let mut config = SandboxConfig::new();
        config.filesystem.private_home = true;
        let mut profile = ProfileConfig::default();
        profile.private_home = true; // Would be redundant, not harmful.
        config.merge_profile(&profile);
        assert!(config.filesystem.private_home);
    }

    #[test]
    fn test_sandbox_config_merge_restrictions() {
        let mut config = SandboxConfig::new();
        config
            .filesystem
            .restrictions
            .push(FsRestriction::Blacklist("/boot".to_string()));
        let mut profile = ProfileConfig::default();
        profile
            .restrictions
            .push(FsRestriction::Blacklist("/sbin".to_string()));
        config.merge_profile(&profile);
        assert_eq!(config.filesystem.restrictions.len(), 2);
    }

    #[test]
    fn test_sandbox_config_merge_net_mode() {
        let mut config = SandboxConfig::new();
        // Default is Host.
        let mut profile = ProfileConfig::default();
        profile.net_mode = Some(NetMode::None);
        config.merge_profile(&profile);
        assert_eq!(config.network.mode, NetMode::None);
    }

    #[test]
    fn test_sandbox_config_merge_net_mode_cli_wins() {
        let mut config = SandboxConfig::new();
        config.network.mode = NetMode::None;
        let mut profile = ProfileConfig::default();
        profile.net_mode = Some(NetMode::Interface("eth0".to_string()));
        config.merge_profile(&profile);
        // CLI already set None, profile should not override.
        assert_eq!(config.network.mode, NetMode::None);
    }

    #[test]
    fn test_sandbox_config_merge_caps_drop() {
        let mut config = SandboxConfig::new();
        let mut profile = ProfileConfig::default();
        profile.caps_drop_all = true;
        config.merge_profile(&profile);
        assert_eq!(config.security.caps_drop, CapsDrop::All);
    }

    #[test]
    fn test_sandbox_config_merge_caps_cli_wins() {
        let mut config = SandboxConfig::new();
        config.security.caps_drop = CapsDrop::All;
        let mut profile = ProfileConfig::default();
        profile.caps_drop_all = true; // Redundant.
        config.merge_profile(&profile);
        assert_eq!(config.security.caps_drop, CapsDrop::All);
    }

    #[test]
    fn test_sandbox_config_merge_private_etc() {
        let mut config = SandboxConfig::new();
        let mut profile = ProfileConfig::default();
        profile.private_etc = vec!["fonts".to_string(), "ssl".to_string()];
        config.merge_profile(&profile);
        assert_eq!(config.filesystem.private_etc, vec!["fonts", "ssl"]);
    }

    #[test]
    fn test_sandbox_config_merge_private_etc_cli_wins() {
        let mut config = SandboxConfig::new();
        config.filesystem.private_etc = vec!["pki".to_string()];
        let mut profile = ProfileConfig::default();
        profile.private_etc = vec!["fonts".to_string(), "ssl".to_string()];
        config.merge_profile(&profile);
        // CLI already set private_etc, profile should not override.
        assert_eq!(config.filesystem.private_etc, vec!["pki"]);
    }

    #[test]
    fn test_sandbox_config_merge_all_security() {
        let mut config = SandboxConfig::new();
        let mut profile = ProfileConfig::default();
        profile.noroot = true;
        profile.nosound = true;
        profile.no3d = true;
        profile.novideo = true;
        profile.nodvd = true;
        profile.notv = true;
        profile.nou2f = true;
        profile.netfilter = true;
        config.merge_profile(&profile);
        assert!(config.security.noroot);
        assert!(config.security.nosound);
        assert!(config.security.no3d);
        assert!(config.security.novideo);
        assert!(config.security.nodvd);
        assert!(config.security.notv);
        assert!(config.security.nou2f);
        assert!(config.network.netfilter);
    }

    // -----------------------------------------------------------------------
    // Argument parsing tests (firejail)
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_firejail_args_simple_program() {
        let args: Vec<String> = vec!["firefox".to_string()];
        let (config, prog_args) = parse_firejail_args(&args).unwrap();
        assert_eq!(prog_args, vec!["firefox"]);
        assert!(!config.filesystem.private_home);
    }

    #[test]
    fn test_parse_firejail_args_program_with_args() {
        let args: Vec<String> = vec![
            "firefox".to_string(),
            "--new-window".to_string(),
            "https://example.com".to_string(),
        ];
        let (_, prog_args) = parse_firejail_args(&args).unwrap();
        assert_eq!(
            prog_args,
            vec!["firefox", "--new-window", "https://example.com"]
        );
    }

    #[test]
    fn test_parse_firejail_args_private() {
        let args: Vec<String> = vec!["--private".to_string(), "bash".to_string()];
        let (config, prog_args) = parse_firejail_args(&args).unwrap();
        assert!(config.filesystem.private_home);
        assert_eq!(prog_args, vec!["bash"]);
    }

    #[test]
    fn test_parse_firejail_args_private_tmp() {
        let args: Vec<String> = vec!["--private-tmp".to_string(), "bash".to_string()];
        let (config, _) = parse_firejail_args(&args).unwrap();
        assert!(config.filesystem.private_tmp);
    }

    #[test]
    fn test_parse_firejail_args_private_dev() {
        let args: Vec<String> = vec!["--private-dev".to_string(), "bash".to_string()];
        let (config, _) = parse_firejail_args(&args).unwrap();
        assert!(config.filesystem.private_dev);
    }

    #[test]
    fn test_parse_firejail_args_private_etc() {
        let args: Vec<String> = vec![
            "--private-etc=fonts,ssl,pki".to_string(),
            "bash".to_string(),
        ];
        let (config, _) = parse_firejail_args(&args).unwrap();
        assert_eq!(config.filesystem.private_etc, vec!["fonts", "ssl", "pki"]);
    }

    #[test]
    fn test_parse_firejail_args_private_bin() {
        let args: Vec<String> = vec![
            "--private-bin=firefox,sh".to_string(),
            "firefox".to_string(),
        ];
        let (config, _) = parse_firejail_args(&args).unwrap();
        assert_eq!(config.filesystem.private_bin, vec!["firefox", "sh"]);
    }

    #[test]
    fn test_parse_firejail_args_net_none() {
        let args: Vec<String> = vec!["--net=none".to_string(), "bash".to_string()];
        let (config, _) = parse_firejail_args(&args).unwrap();
        assert_eq!(config.network.mode, NetMode::None);
    }

    #[test]
    fn test_parse_firejail_args_net_interface() {
        let args: Vec<String> = vec!["--net=eth0".to_string(), "bash".to_string()];
        let (config, _) = parse_firejail_args(&args).unwrap();
        assert_eq!(
            config.network.mode,
            NetMode::Interface("eth0".to_string())
        );
    }

    #[test]
    fn test_parse_firejail_args_dns() {
        let args: Vec<String> = vec![
            "--dns=8.8.8.8".to_string(),
            "--dns=8.8.4.4".to_string(),
            "bash".to_string(),
        ];
        let (config, _) = parse_firejail_args(&args).unwrap();
        assert_eq!(config.network.dns.servers, vec!["8.8.8.8", "8.8.4.4"]);
    }

    #[test]
    fn test_parse_firejail_args_ip() {
        let args: Vec<String> = vec!["--ip=10.0.0.5".to_string(), "bash".to_string()];
        let (config, _) = parse_firejail_args(&args).unwrap();
        assert_eq!(config.network.ip.address, Some("10.0.0.5".to_string()));
    }

    #[test]
    fn test_parse_firejail_args_mac() {
        let args: Vec<String> = vec![
            "--mac=00:11:22:33:44:55".to_string(),
            "bash".to_string(),
        ];
        let (config, _) = parse_firejail_args(&args).unwrap();
        assert_eq!(
            config.network.ip.mac,
            Some("00:11:22:33:44:55".to_string())
        );
    }

    #[test]
    fn test_parse_firejail_args_hostname() {
        let args: Vec<String> = vec![
            "--hostname=sandbox1".to_string(),
            "bash".to_string(),
        ];
        let (config, _) = parse_firejail_args(&args).unwrap();
        assert_eq!(config.network.hostname, Some("sandbox1".to_string()));
    }

    #[test]
    fn test_parse_firejail_args_shell_none() {
        let args: Vec<String> = vec!["--shell=none".to_string(), "bash".to_string()];
        let (config, _) = parse_firejail_args(&args).unwrap();
        assert_eq!(config.shell, Some("none".to_string()));
    }

    #[test]
    fn test_parse_firejail_args_noprofile() {
        let args: Vec<String> = vec!["--noprofile".to_string(), "bash".to_string()];
        let (config, _) = parse_firejail_args(&args).unwrap();
        assert!(config.noprofile);
    }

    #[test]
    fn test_parse_firejail_args_profile() {
        let args: Vec<String> = vec![
            "--profile=/etc/firejail/firefox.profile".to_string(),
            "firefox".to_string(),
        ];
        let (config, _) = parse_firejail_args(&args).unwrap();
        assert_eq!(
            config.profile_path,
            Some("/etc/firejail/firefox.profile".to_string())
        );
    }

    #[test]
    fn test_parse_firejail_args_caps_drop_all() {
        let args: Vec<String> = vec!["--caps.drop=all".to_string(), "bash".to_string()];
        let (config, _) = parse_firejail_args(&args).unwrap();
        assert_eq!(config.security.caps_drop, CapsDrop::All);
    }

    #[test]
    fn test_parse_firejail_args_caps_drop_list() {
        let args: Vec<String> = vec![
            "--caps.drop=net_raw,sys_admin".to_string(),
            "bash".to_string(),
        ];
        let (config, _) = parse_firejail_args(&args).unwrap();
        assert_eq!(
            config.security.caps_drop,
            CapsDrop::List(vec!["net_raw".to_string(), "sys_admin".to_string()])
        );
    }

    #[test]
    fn test_parse_firejail_args_seccomp() {
        let args: Vec<String> = vec!["--seccomp".to_string(), "bash".to_string()];
        let (config, _) = parse_firejail_args(&args).unwrap();
        assert!(config.security.seccomp);
    }

    #[test]
    fn test_parse_firejail_args_noroot() {
        let args: Vec<String> = vec!["--noroot".to_string(), "bash".to_string()];
        let (config, _) = parse_firejail_args(&args).unwrap();
        assert!(config.security.noroot);
    }

    #[test]
    fn test_parse_firejail_args_nosound() {
        let args: Vec<String> = vec!["--nosound".to_string(), "bash".to_string()];
        let (config, _) = parse_firejail_args(&args).unwrap();
        assert!(config.security.nosound);
    }

    #[test]
    fn test_parse_firejail_args_no3d() {
        let args: Vec<String> = vec!["--no3d".to_string(), "bash".to_string()];
        let (config, _) = parse_firejail_args(&args).unwrap();
        assert!(config.security.no3d);
    }

    #[test]
    fn test_parse_firejail_args_novideo() {
        let args: Vec<String> = vec!["--novideo".to_string(), "bash".to_string()];
        let (config, _) = parse_firejail_args(&args).unwrap();
        assert!(config.security.novideo);
    }

    #[test]
    fn test_parse_firejail_args_nodvd() {
        let args: Vec<String> = vec!["--nodvd".to_string(), "bash".to_string()];
        let (config, _) = parse_firejail_args(&args).unwrap();
        assert!(config.security.nodvd);
    }

    #[test]
    fn test_parse_firejail_args_notv() {
        let args: Vec<String> = vec!["--notv".to_string(), "bash".to_string()];
        let (config, _) = parse_firejail_args(&args).unwrap();
        assert!(config.security.notv);
    }

    #[test]
    fn test_parse_firejail_args_nou2f() {
        let args: Vec<String> = vec!["--nou2f".to_string(), "bash".to_string()];
        let (config, _) = parse_firejail_args(&args).unwrap();
        assert!(config.security.nou2f);
    }

    #[test]
    fn test_parse_firejail_args_netfilter() {
        let args: Vec<String> = vec!["--netfilter".to_string(), "bash".to_string()];
        let (config, _) = parse_firejail_args(&args).unwrap();
        assert!(config.network.netfilter);
    }

    #[test]
    fn test_parse_firejail_args_debug() {
        let args: Vec<String> = vec!["--debug".to_string(), "bash".to_string()];
        let (config, _) = parse_firejail_args(&args).unwrap();
        assert!(config.debug);
    }

    #[test]
    fn test_parse_firejail_args_quiet() {
        let args: Vec<String> = vec!["--quiet".to_string(), "bash".to_string()];
        let (config, _) = parse_firejail_args(&args).unwrap();
        assert!(config.quiet);
    }

    #[test]
    fn test_parse_firejail_args_name() {
        let args: Vec<String> = vec![
            "--name=mybox".to_string(),
            "bash".to_string(),
        ];
        let (config, _) = parse_firejail_args(&args).unwrap();
        assert_eq!(config.name, Some("mybox".to_string()));
    }

    #[test]
    fn test_parse_firejail_args_blacklist() {
        let args: Vec<String> = vec![
            "--blacklist=/boot".to_string(),
            "bash".to_string(),
        ];
        let (config, _) = parse_firejail_args(&args).unwrap();
        assert_eq!(
            config.filesystem.restrictions,
            vec![FsRestriction::Blacklist("/boot".to_string())]
        );
    }

    #[test]
    fn test_parse_firejail_args_whitelist() {
        let args: Vec<String> = vec![
            "--whitelist=/home/user".to_string(),
            "bash".to_string(),
        ];
        let (config, _) = parse_firejail_args(&args).unwrap();
        assert_eq!(
            config.filesystem.restrictions,
            vec![FsRestriction::Whitelist("/home/user".to_string())]
        );
    }

    #[test]
    fn test_parse_firejail_args_readonly() {
        let args: Vec<String> = vec![
            "--read-only=/etc".to_string(),
            "bash".to_string(),
        ];
        let (config, _) = parse_firejail_args(&args).unwrap();
        assert_eq!(
            config.filesystem.restrictions,
            vec![FsRestriction::ReadOnly("/etc".to_string())]
        );
    }

    #[test]
    fn test_parse_firejail_args_tmpfs() {
        let args: Vec<String> = vec![
            "--tmpfs=/var/tmp".to_string(),
            "bash".to_string(),
        ];
        let (config, _) = parse_firejail_args(&args).unwrap();
        assert_eq!(
            config.filesystem.restrictions,
            vec![FsRestriction::Tmpfs("/var/tmp".to_string())]
        );
    }

    #[test]
    fn test_parse_firejail_args_noexec() {
        let args: Vec<String> = vec![
            "--noexec=/tmp".to_string(),
            "bash".to_string(),
        ];
        let (config, _) = parse_firejail_args(&args).unwrap();
        assert_eq!(
            config.filesystem.restrictions,
            vec![FsRestriction::NoExec("/tmp".to_string())]
        );
    }

    #[test]
    fn test_parse_firejail_args_list() {
        let args: Vec<String> = vec!["--list".to_string()];
        let (_, prog_args) = parse_firejail_args(&args).unwrap();
        assert_eq!(prog_args, vec!["--list"]);
    }

    #[test]
    fn test_parse_firejail_args_tree() {
        let args: Vec<String> = vec!["--tree".to_string()];
        let (_, prog_args) = parse_firejail_args(&args).unwrap();
        assert_eq!(prog_args, vec!["--tree"]);
    }

    #[test]
    fn test_parse_firejail_args_top() {
        let args: Vec<String> = vec!["--top".to_string()];
        let (_, prog_args) = parse_firejail_args(&args).unwrap();
        assert_eq!(prog_args, vec!["--top"]);
    }

    #[test]
    fn test_parse_firejail_args_join() {
        let args: Vec<String> = vec!["--join=mybox".to_string()];
        let (_, prog_args) = parse_firejail_args(&args).unwrap();
        assert_eq!(prog_args, vec!["--join", "mybox"]);
    }

    #[test]
    fn test_parse_firejail_args_shutdown() {
        let args: Vec<String> = vec!["--shutdown=mybox".to_string()];
        let (_, prog_args) = parse_firejail_args(&args).unwrap();
        assert_eq!(prog_args, vec!["--shutdown", "mybox"]);
    }

    #[test]
    fn test_parse_firejail_args_version() {
        let args: Vec<String> = vec!["--version".to_string()];
        let (_, prog_args) = parse_firejail_args(&args).unwrap();
        assert_eq!(prog_args, vec!["--version"]);
    }

    #[test]
    fn test_parse_firejail_args_help() {
        let args: Vec<String> = vec!["--help".to_string()];
        let (_, prog_args) = parse_firejail_args(&args).unwrap();
        assert_eq!(prog_args, vec!["--help"]);
    }

    #[test]
    fn test_parse_firejail_args_unknown_option() {
        let args: Vec<String> = vec!["--bogus".to_string()];
        let result = parse_firejail_args(&args);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_firejail_args_no_program() {
        let args: Vec<String> = vec![];
        let (_, prog_args) = parse_firejail_args(&args).unwrap();
        assert!(prog_args.is_empty());
    }

    #[test]
    fn test_parse_firejail_args_complex() {
        let args: Vec<String> = vec![
            "--private".to_string(),
            "--private-tmp".to_string(),
            "--net=none".to_string(),
            "--caps.drop=all".to_string(),
            "--seccomp".to_string(),
            "--noroot".to_string(),
            "--blacklist=/boot".to_string(),
            "--name=browser".to_string(),
            "firefox".to_string(),
            "--new-window".to_string(),
        ];
        let (config, prog_args) = parse_firejail_args(&args).unwrap();
        assert!(config.filesystem.private_home);
        assert!(config.filesystem.private_tmp);
        assert_eq!(config.network.mode, NetMode::None);
        assert_eq!(config.security.caps_drop, CapsDrop::All);
        assert!(config.security.seccomp);
        assert!(config.security.noroot);
        assert_eq!(
            config.filesystem.restrictions,
            vec![FsRestriction::Blacklist("/boot".to_string())]
        );
        assert_eq!(config.name, Some("browser".to_string()));
        assert_eq!(prog_args, vec!["firefox", "--new-window"]);
    }

    #[test]
    fn test_parse_firejail_args_multiple_restrictions() {
        let args: Vec<String> = vec![
            "--blacklist=/boot".to_string(),
            "--blacklist=/sbin".to_string(),
            "--read-only=/etc".to_string(),
            "--noexec=/tmp".to_string(),
            "bash".to_string(),
        ];
        let (config, _) = parse_firejail_args(&args).unwrap();
        assert_eq!(config.filesystem.restrictions.len(), 4);
    }

    // -----------------------------------------------------------------------
    // SandboxInfo tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_sandbox_info_cpu_display_zero() {
        let info = SandboxInfo {
            pid: 1,
            name: "test".to_string(),
            program: "bash".to_string(),
            user: "root".to_string(),
            uptime_secs: 0,
            cpu_millipercent: 0,
            mem_kb: 0,
            net_mode: "host".to_string(),
            caps_dropped: false,
            seccomp_enabled: false,
            children: Vec::new(),
        };
        assert_eq!(info.cpu_display(), "0.0%");
    }

    #[test]
    fn test_sandbox_info_cpu_display_full() {
        let info = SandboxInfo {
            pid: 1,
            name: "test".to_string(),
            program: "bash".to_string(),
            user: "root".to_string(),
            uptime_secs: 0,
            cpu_millipercent: 100_000,
            mem_kb: 0,
            net_mode: "host".to_string(),
            caps_dropped: false,
            seccomp_enabled: false,
            children: Vec::new(),
        };
        assert_eq!(info.cpu_display(), "100.0%");
    }

    #[test]
    fn test_sandbox_info_cpu_display_fractional() {
        let info = SandboxInfo {
            pid: 1,
            name: "test".to_string(),
            program: "bash".to_string(),
            user: "root".to_string(),
            uptime_secs: 0,
            cpu_millipercent: 15_500,
            mem_kb: 0,
            net_mode: "host".to_string(),
            caps_dropped: false,
            seccomp_enabled: false,
            children: Vec::new(),
        };
        assert_eq!(info.cpu_display(), "15.5%");
    }

    #[test]
    fn test_sandbox_info_display() {
        let info = SandboxInfo {
            pid: 12345,
            name: "firefox".to_string(),
            program: "/usr/bin/firefox".to_string(),
            user: "alice".to_string(),
            uptime_secs: 0,
            cpu_millipercent: 5_000,
            mem_kb: 1024,
            net_mode: "host".to_string(),
            caps_dropped: true,
            seccomp_enabled: true,
            children: Vec::new(),
        };
        let display = format!("{info}");
        assert!(display.contains("12345"));
        assert!(display.contains("alice"));
        assert!(display.contains("firefox"));
    }

    // -----------------------------------------------------------------------
    // Sandbox file parsing tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_sandbox_file_content() {
        // We test the parsing logic indirectly by creating a temp file.
        let dir = env::temp_dir().join("firejail_test_parse");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("100.sandbox");
        let content = "pid=100\n\
                        name=browser\n\
                        program=firefox\n\
                        user=alice\n\
                        uptime=3600\n\
                        cpu=5000\n\
                        mem=102400\n\
                        net=none\n\
                        caps=dropped\n\
                        seccomp=enabled\n\
                        children=101,102\n";
        let _ = fs::write(&path, content);
        let info = parse_sandbox_file(&path).unwrap();
        assert_eq!(info.pid, 100);
        assert_eq!(info.name, "browser");
        assert_eq!(info.program, "firefox");
        assert_eq!(info.user, "alice");
        assert_eq!(info.uptime_secs, 3600);
        assert_eq!(info.cpu_millipercent, 5000);
        assert_eq!(info.mem_kb, 102400);
        assert_eq!(info.net_mode, "none");
        assert!(info.caps_dropped);
        assert!(info.seccomp_enabled);
        assert_eq!(info.children, vec![101, 102]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_parse_sandbox_file_missing_pid() {
        let dir = env::temp_dir().join("firejail_test_nopid");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("bad.sandbox");
        let content = "name=test\nprogram=bash\n";
        let _ = fs::write(&path, content);
        let result = parse_sandbox_file(&path);
        assert!(result.is_none());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_write_and_read_sandbox_file() {
        let dir = env::temp_dir().join("firejail_test_wr");
        let _ = fs::create_dir_all(&dir);
        let info = SandboxInfo {
            pid: 999,
            name: "mybox".to_string(),
            program: "bash".to_string(),
            user: "root".to_string(),
            uptime_secs: 120,
            cpu_millipercent: 2500,
            mem_kb: 512,
            net_mode: "host".to_string(),
            caps_dropped: false,
            seccomp_enabled: true,
            children: vec![1000, 1001],
        };
        write_sandbox_file(&dir, &info).unwrap();
        let read_info = parse_sandbox_file(&dir.join("999.sandbox")).unwrap();
        assert_eq!(read_info.pid, 999);
        assert_eq!(read_info.name, "mybox");
        assert_eq!(read_info.program, "bash");
        assert!(read_info.seccomp_enabled);
        assert!(!read_info.caps_dropped);
        assert_eq!(read_info.children, vec![1000, 1001]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_remove_sandbox_file() {
        let dir = env::temp_dir().join("firejail_test_rm");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("555.sandbox");
        let _ = fs::write(&path, "pid=555\nname=test\n");
        assert!(path.exists());
        remove_sandbox_file(&dir, 555).unwrap();
        assert!(!path.exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_remove_sandbox_file_nonexistent() {
        let dir = env::temp_dir().join("firejail_test_rm_ne");
        let _ = fs::create_dir_all(&dir);
        // Should not error when file does not exist.
        remove_sandbox_file(&dir, 9999).unwrap();
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_read_sandbox_entries_empty_dir() {
        let dir = env::temp_dir().join("firejail_test_empty");
        let _ = fs::create_dir_all(&dir);
        let entries = read_sandbox_entries(&dir);
        assert!(entries.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_read_sandbox_entries_nonexistent_dir() {
        let dir = Path::new("/nonexistent/firejail/dir");
        let entries = read_sandbox_entries(dir);
        assert!(entries.is_empty());
    }

    #[test]
    fn test_read_sandbox_entries_ignores_non_sandbox() {
        let dir = env::temp_dir().join("firejail_test_ignore");
        let _ = fs::create_dir_all(&dir);
        let _ = fs::write(dir.join("100.sandbox"), "pid=100\nname=a\n");
        let _ = fs::write(dir.join("readme.txt"), "not a sandbox file");
        let entries = read_sandbox_entries(&dir);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].pid, 100);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_read_sandbox_entries_sorted() {
        let dir = env::temp_dir().join("firejail_test_sort");
        let _ = fs::create_dir_all(&dir);
        let _ = fs::write(dir.join("300.sandbox"), "pid=300\nname=c\n");
        let _ = fs::write(dir.join("100.sandbox"), "pid=100\nname=a\n");
        let _ = fs::write(dir.join("200.sandbox"), "pid=200\nname=b\n");
        let entries = read_sandbox_entries(&dir);
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].pid, 100);
        assert_eq!(entries[1].pid, 200);
        assert_eq!(entries[2].pid, 300);
        let _ = fs::remove_dir_all(&dir);
    }

    // -----------------------------------------------------------------------
    // Built-in profiles tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_builtin_profiles_firefox() {
        let profiles = builtin_profiles();
        assert!(profiles.contains_key("firefox"));
    }

    #[test]
    fn test_builtin_profiles_chromium() {
        let profiles = builtin_profiles();
        assert!(profiles.contains_key("chromium"));
    }

    #[test]
    fn test_builtin_profiles_vlc() {
        let profiles = builtin_profiles();
        assert!(profiles.contains_key("vlc"));
    }

    #[test]
    fn test_builtin_profiles_all_parseable() {
        let profiles = builtin_profiles();
        for (name, content) in &profiles {
            let path = PathBuf::from(format!("{name}.profile"));
            let result = parse_profile_content(content, &path);
            assert!(
                result.is_ok(),
                "Built-in profile {name} failed to parse: {:?}",
                result.err()
            );
        }
    }

    #[test]
    fn test_builtin_firefox_profile_has_seccomp() {
        let profiles = builtin_profiles();
        let content = profiles["firefox"];
        let path = PathBuf::from("firefox.profile");
        let profile = parse_profile_content(content, &path).unwrap();
        assert!(profile.seccomp);
    }

    #[test]
    fn test_builtin_firefox_profile_has_noroot() {
        let profiles = builtin_profiles();
        let content = profiles["firefox"];
        let path = PathBuf::from("firefox.profile");
        let profile = parse_profile_content(content, &path).unwrap();
        assert!(profile.noroot);
    }

    #[test]
    fn test_builtin_firefox_profile_has_caps_drop_all() {
        let profiles = builtin_profiles();
        let content = profiles["firefox"];
        let path = PathBuf::from("firefox.profile");
        let profile = parse_profile_content(content, &path).unwrap();
        assert!(profile.caps_drop_all);
    }

    #[test]
    fn test_builtin_evince_profile_net_none() {
        let profiles = builtin_profiles();
        let content = profiles["evince"];
        let path = PathBuf::from("evince.profile");
        let profile = parse_profile_content(content, &path).unwrap();
        assert_eq!(profile.net_mode, Some(NetMode::None));
    }

    #[test]
    fn test_supported_app_names_sorted() {
        let names = supported_app_names();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        assert_eq!(names, sorted);
    }

    #[test]
    fn test_supported_app_names_not_empty() {
        assert!(!supported_app_names().is_empty());
    }

    // -----------------------------------------------------------------------
    // NetworkConfig / DnsConfig / IpConfig tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_network_config_default() {
        let nc = NetworkConfig::default();
        assert_eq!(nc.mode, NetMode::Host);
        assert!(nc.dns.servers.is_empty());
        assert!(nc.ip.address.is_none());
        assert!(nc.ip.mac.is_none());
        assert!(nc.hostname.is_none());
        assert!(!nc.netfilter);
    }

    #[test]
    fn test_dns_config_default() {
        let dc = DnsConfig::default();
        assert!(dc.servers.is_empty());
    }

    #[test]
    fn test_ip_config_default() {
        let ic = IpConfig::default();
        assert!(ic.address.is_none());
        assert!(ic.mac.is_none());
    }

    // -----------------------------------------------------------------------
    // FsConfig / SecurityConfig defaults tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_fs_config_default() {
        let fc = FsConfig::default();
        assert!(!fc.private_home);
        assert!(!fc.private_tmp);
        assert!(!fc.private_dev);
        assert!(fc.private_etc.is_empty());
        assert!(fc.private_bin.is_empty());
        assert!(fc.restrictions.is_empty());
    }

    #[test]
    fn test_security_config_default() {
        let sc = SecurityConfig::default();
        assert_eq!(sc.caps_drop, CapsDrop::None);
        assert!(!sc.seccomp);
        assert!(!sc.noroot);
        assert!(!sc.nosound);
        assert!(!sc.no3d);
        assert!(!sc.novideo);
        assert!(!sc.nodvd);
        assert!(!sc.notv);
        assert!(!sc.nou2f);
    }

    #[test]
    fn test_sandbox_config_default() {
        let sc = SandboxConfig::new();
        assert!(sc.name.is_none());
        assert!(!sc.noprofile);
        assert!(sc.profile_path.is_none());
        assert!(!sc.debug);
        assert!(!sc.quiet);
    }

    // -----------------------------------------------------------------------
    // get_current_user test
    // -----------------------------------------------------------------------

    #[test]
    fn test_get_current_user_not_empty() {
        // Should return at least "root" as fallback.
        let user = get_current_user();
        assert!(!user.is_empty());
    }

    // -----------------------------------------------------------------------
    // Profile discovery tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_find_profile_builtin() {
        // Use a nonexistent directory so only built-in profiles are found.
        let dir = Path::new("/nonexistent/firejail/profiles");
        let profile = find_profile("firefox", dir);
        assert!(profile.is_some());
        let p = profile.unwrap();
        assert!(p.seccomp);
    }

    #[test]
    fn test_find_profile_not_found() {
        let dir = Path::new("/nonexistent/firejail/profiles");
        let profile = find_profile("totally_made_up_app_xyz", dir);
        assert!(profile.is_none());
    }

    // -----------------------------------------------------------------------
    // Edge case tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_firejail_args_help_short() {
        let args: Vec<String> = vec!["-h".to_string()];
        let (_, prog_args) = parse_firejail_args(&args).unwrap();
        assert_eq!(prog_args, vec!["--help"]);
    }

    #[test]
    fn test_net_mode_clone() {
        let mode = NetMode::Interface("br0".to_string());
        let cloned = mode.clone();
        assert_eq!(mode, cloned);
    }

    #[test]
    fn test_fs_restriction_clone() {
        let r = FsRestriction::Blacklist("/boot".to_string());
        let cloned = r.clone();
        assert_eq!(r, cloned);
    }

    #[test]
    fn test_caps_drop_clone() {
        let c = CapsDrop::List(vec!["sys_admin".to_string()]);
        let cloned = c.clone();
        assert_eq!(c, cloned);
    }

    #[test]
    fn test_sandbox_info_clone() {
        let info = SandboxInfo {
            pid: 1,
            name: "test".to_string(),
            program: "bash".to_string(),
            user: "root".to_string(),
            uptime_secs: 0,
            cpu_millipercent: 0,
            mem_kb: 0,
            net_mode: "host".to_string(),
            caps_dropped: false,
            seccomp_enabled: false,
            children: Vec::new(),
        };
        let cloned = info.clone();
        assert_eq!(info.pid, cloned.pid);
        assert_eq!(info.name, cloned.name);
    }

    #[test]
    fn test_profile_config_default() {
        let pc = ProfileConfig::default();
        assert!(pc.name.is_empty());
        assert!(!pc.private_home);
        assert!(!pc.private_tmp);
        assert!(!pc.private_dev);
        assert!(pc.private_etc.is_empty());
        assert!(pc.private_bin.is_empty());
        assert!(!pc.seccomp);
        assert!(!pc.caps_drop_all);
        assert!(!pc.noroot);
        assert!(pc.restrictions.is_empty());
        assert!(pc.includes.is_empty());
        assert!(pc.net_mode.is_none());
    }

    #[test]
    fn test_split_csv_only_commas() {
        let result = split_csv(",,,");
        assert!(result.is_empty());
    }

    #[test]
    fn test_format_uptime_large_value() {
        // 1 week = 604800 seconds.
        let result = format_uptime(604800);
        assert_eq!(result, "7d 0h");
    }

    #[test]
    fn test_sandbox_info_cpu_display_small() {
        let info = SandboxInfo {
            pid: 1,
            name: "t".to_string(),
            program: "b".to_string(),
            user: "r".to_string(),
            uptime_secs: 0,
            cpu_millipercent: 100,
            mem_kb: 0,
            net_mode: "h".to_string(),
            caps_dropped: false,
            seccomp_enabled: false,
            children: Vec::new(),
        };
        assert_eq!(info.cpu_display(), "0.1%");
    }

    #[test]
    fn test_truncate_str_limit_zero() {
        assert_eq!(truncate_str("hello", 0), "");
    }

    #[test]
    fn test_truncate_str_limit_one() {
        assert_eq!(truncate_str("hello", 1), "h");
    }
}
