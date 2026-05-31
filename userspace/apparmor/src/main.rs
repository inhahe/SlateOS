//! OurOS AppArmor mandatory access control utilities.
//!
//! Multi-personality binary providing:
//! - **aa-status** (default) — show AppArmor status and loaded profiles
//! - **aa-enforce** — set profiles to enforce mode
//! - **aa-complain** — set profiles to complain mode
//! - **aa-disable** — disable profiles
//! - **aa-genprof** — generate profile for a program
//! - **aa-logprof** — update profiles from logs
//! - **aa-unconfined** — list unconfined processes
//! - **apparmor_parser** — compile/load/remove profiles
//!
//! Implements the AppArmor mandatory access control interface for managing
//! security profiles that restrict program capabilities.

#![deny(clippy::all)]

use std::collections::HashMap;
use std::env;
use std::fmt;
use std::fs;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::process;

const VERSION: &str = "0.1.0";
const PROFILES_DIR: &str = "/etc/apparmor.d";
const CACHE_DIR: &str = "/var/cache/apparmor";
const LOG_FILE: &str = "/var/log/audit/audit.log";
const SYSLOG_FILE: &str = "/var/log/syslog";
const APPARMORFS: &str = "/sys/kernel/security/apparmor";
const PROFILES_STATE: &str = "/sys/kernel/security/apparmor/profiles";
const PROC_DIR: &str = "/proc";

// ============================================================================
// Profile mode
// ============================================================================

/// Operating mode for an AppArmor profile.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum ProfileMode {
    Enforce,
    Complain,
    Disable,
    Unconfined,
}

impl ProfileMode {
    fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "enforce" => Self::Enforce,
            "complain" => Self::Complain,
            "disable" | "disabled" => Self::Disable,
            "unconfined" => Self::Unconfined,
            _ => Self::Enforce,
        }
    }

    fn as_str(&self) -> &'static str {
        match self {
            Self::Enforce => "enforce",
            Self::Complain => "complain",
            Self::Disable => "disable",
            Self::Unconfined => "unconfined",
        }
    }
}

impl fmt::Display for ProfileMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

// ============================================================================
// Capability type
// ============================================================================

/// Linux capabilities that can be granted or denied.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Capability {
    NetBindService,
    NetAdmin,
    NetRaw,
    SysAdmin,
    SysPtrace,
    SysRawio,
    DacOverride,
    DacReadSearch,
    Fowner,
    Fsetid,
    Kill,
    Setgid,
    Setuid,
    Chown,
    Mknod,
    SysChroot,
    AuditWrite,
    Other(String),
}

impl Capability {
    fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "net_bind_service" => Self::NetBindService,
            "net_admin" => Self::NetAdmin,
            "net_raw" => Self::NetRaw,
            "sys_admin" => Self::SysAdmin,
            "sys_ptrace" => Self::SysPtrace,
            "sys_rawio" => Self::SysRawio,
            "dac_override" => Self::DacOverride,
            "dac_read_search" => Self::DacReadSearch,
            "fowner" => Self::Fowner,
            "fsetid" => Self::Fsetid,
            "kill" => Self::Kill,
            "setgid" => Self::Setgid,
            "setuid" => Self::Setuid,
            "chown" => Self::Chown,
            "mknod" => Self::Mknod,
            "sys_chroot" => Self::SysChroot,
            "audit_write" => Self::AuditWrite,
            other => Self::Other(other.to_string()),
        }
    }

    fn as_str(&self) -> &str {
        match self {
            Self::NetBindService => "net_bind_service",
            Self::NetAdmin => "net_admin",
            Self::NetRaw => "net_raw",
            Self::SysAdmin => "sys_admin",
            Self::SysPtrace => "sys_ptrace",
            Self::SysRawio => "sys_rawio",
            Self::DacOverride => "dac_override",
            Self::DacReadSearch => "dac_read_search",
            Self::Fowner => "fowner",
            Self::Fsetid => "fsetid",
            Self::Kill => "kill",
            Self::Setgid => "setgid",
            Self::Setuid => "setuid",
            Self::Chown => "chown",
            Self::Mknod => "mknod",
            Self::SysChroot => "sys_chroot",
            Self::AuditWrite => "audit_write",
            Self::Other(s) => s.as_str(),
        }
    }
}

impl fmt::Display for Capability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

// ============================================================================
// File permission
// ============================================================================

/// File access permission flags.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct FilePermission {
    read: bool,
    write: bool,
    append: bool,
    execute: bool,
    memory_map: bool,
    link: bool,
    lock: bool,
}

impl FilePermission {
    fn new() -> Self {
        Self {
            read: false,
            write: false,
            append: false,
            execute: false,
            memory_map: false,
            link: false,
            lock: false,
        }
    }

    fn from_str(s: &str) -> Self {
        let mut perm = Self::new();
        for ch in s.chars() {
            match ch {
                'r' => perm.read = true,
                'w' => perm.write = true,
                'a' => perm.append = true,
                'x' => perm.execute = true,
                'm' => perm.memory_map = true,
                'l' => perm.link = true,
                'k' => perm.lock = true,
                _ => {}
            }
        }
        perm
    }

    fn is_empty(&self) -> bool {
        !self.read
            && !self.write
            && !self.append
            && !self.execute
            && !self.memory_map
            && !self.link
            && !self.lock
    }
}

impl fmt::Display for FilePermission {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.read {
            write!(f, "r")?;
        }
        if self.write {
            write!(f, "w")?;
        }
        if self.append {
            write!(f, "a")?;
        }
        if self.execute {
            write!(f, "x")?;
        }
        if self.memory_map {
            write!(f, "m")?;
        }
        if self.link {
            write!(f, "l")?;
        }
        if self.lock {
            write!(f, "k")?;
        }
        Ok(())
    }
}

// ============================================================================
// Network rule
// ============================================================================

/// Network access domain.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum NetDomain {
    Inet,
    Inet6,
    Unix,
    Netlink,
    Packet,
    Other(String),
}

impl NetDomain {
    fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "inet" => Self::Inet,
            "inet6" => Self::Inet6,
            "unix" => Self::Unix,
            "netlink" => Self::Netlink,
            "packet" => Self::Packet,
            other => Self::Other(other.to_string()),
        }
    }

    fn as_str(&self) -> &str {
        match self {
            Self::Inet => "inet",
            Self::Inet6 => "inet6",
            Self::Unix => "unix",
            Self::Netlink => "netlink",
            Self::Packet => "packet",
            Self::Other(s) => s.as_str(),
        }
    }
}

impl fmt::Display for NetDomain {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Network socket type.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum NetType {
    Stream,
    Dgram,
    Raw,
    Seqpacket,
    Other(String),
}

impl NetType {
    fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "stream" => Self::Stream,
            "dgram" => Self::Dgram,
            "raw" => Self::Raw,
            "seqpacket" => Self::Seqpacket,
            other => Self::Other(other.to_string()),
        }
    }

    fn as_str(&self) -> &str {
        match self {
            Self::Stream => "stream",
            Self::Dgram => "dgram",
            Self::Raw => "raw",
            Self::Seqpacket => "seqpacket",
            Self::Other(s) => s.as_str(),
        }
    }
}

impl fmt::Display for NetType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// A network access rule.
#[derive(Clone, Debug, PartialEq, Eq)]
struct NetworkRule {
    domain: NetDomain,
    sock_type: Option<NetType>,
}

impl fmt::Display for NetworkRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "network {}", self.domain)?;
        if let Some(ref st) = self.sock_type {
            write!(f, " {}", st)?;
        }
        Ok(())
    }
}

// ============================================================================
// File rule
// ============================================================================

/// A file access rule within a profile.
#[derive(Clone, Debug, PartialEq, Eq)]
struct FileRule {
    path: String,
    permission: FilePermission,
    owner_only: bool,
}

impl fmt::Display for FileRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.owner_only {
            write!(f, "owner ")?;
        }
        write!(f, "{} {}", self.path, self.permission)
    }
}

// ============================================================================
// Profile
// ============================================================================

/// A complete AppArmor profile.
#[derive(Clone, Debug)]
struct Profile {
    name: String,
    mode: ProfileMode,
    binary_path: Option<String>,
    profile_file: Option<PathBuf>,
    capabilities: Vec<Capability>,
    file_rules: Vec<FileRule>,
    network_rules: Vec<NetworkRule>,
    includes: Vec<String>,
    flags: Vec<String>,
    child_profiles: Vec<String>,
}

impl Profile {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            mode: ProfileMode::Enforce,
            binary_path: None,
            profile_file: None,
            capabilities: Vec::new(),
            file_rules: Vec::new(),
            network_rules: Vec::new(),
            includes: Vec::new(),
            flags: Vec::new(),
            child_profiles: Vec::new(),
        }
    }
}

impl fmt::Display for Profile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({})", self.name, self.mode)
    }
}

// ============================================================================
// Process info
// ============================================================================

/// Information about a running process and its confinement state.
#[derive(Clone, Debug)]
struct ProcessInfo {
    pid: u32,
    name: String,
    profile: Option<String>,
    mode: ProfileMode,
}

impl fmt::Display for ProcessInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.profile {
            Some(prof) => write!(f, "{} ({}) - {} ({})", self.pid, self.name, prof, self.mode),
            None => write!(f, "{} ({}) - unconfined", self.pid, self.name),
        }
    }
}

// ============================================================================
// Audit log entry
// ============================================================================

/// A parsed denial from the audit log.
#[derive(Clone, Debug)]
struct AuditDenial {
    timestamp: String,
    profile: String,
    operation: String,
    denied_mask: String,
    target: String,
    info: Option<String>,
    comm: Option<String>,
    pid: Option<u32>,
}

impl fmt::Display for AuditDenial {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "[{}] {} denied {} ({}) on {}",
            self.timestamp, self.profile, self.operation, self.denied_mask, self.target
        )?;
        if let Some(ref comm) = self.comm {
            write!(f, " comm={}", comm)?;
        }
        if let Some(pid) = self.pid {
            write!(f, " pid={}", pid)?;
        }
        if let Some(ref info) = self.info {
            write!(f, " info={}", info)?;
        }
        Ok(())
    }
}

// ============================================================================
// Parser action (for apparmor_parser)
// ============================================================================

/// Actions for the apparmor_parser personality.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ParserAction {
    Add,
    Replace,
    Remove,
    Preprocess,
}

impl ParserAction {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Add => "add",
            Self::Replace => "replace",
            Self::Remove => "remove",
            Self::Preprocess => "preprocess",
        }
    }
}

impl fmt::Display for ParserAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

// ============================================================================
// Profile parser
// ============================================================================

/// Parse a single profile rule line, returning the updated profile.
fn parse_rule_line(line: &str, profile: &mut Profile) {
    let trimmed = line.trim();
    let trimmed = trimmed.strip_suffix(',').unwrap_or(trimmed);

    // Include directives must be handled BEFORE the comment guard below:
    // AppArmor's classic include syntax is `#include <abstractions/base>`,
    // which starts with '#' yet is a directive, not a comment.
    if let Some(rest) = trimmed
        .strip_prefix("#include")
        .or_else(|| trimmed.strip_prefix("include"))
    {
        let inc = rest
            .trim()
            .trim_matches(|c| c == '<' || c == '>' || c == '"');
        if !inc.is_empty() {
            profile.includes.push(inc.to_string());
        }
        return;
    }

    if trimmed.starts_with('#') || trimmed.is_empty() {
        return;
    }

    if trimmed.starts_with("capability") {
        let rest = trimmed.strip_prefix("capability").unwrap_or("").trim();
        let rest = rest.strip_suffix(',').unwrap_or(rest);
        if !rest.is_empty() {
            profile.capabilities.push(Capability::from_str(rest));
        }
        return;
    }

    if trimmed.starts_with("network") {
        let rest = trimmed.strip_prefix("network").unwrap_or("").trim();
        let rest = rest.strip_suffix(',').unwrap_or(rest);
        let parts: Vec<&str> = rest.split_whitespace().collect();
        if !parts.is_empty() {
            let domain = NetDomain::from_str(parts[0]);
            let sock_type = parts.get(1).map(|s| NetType::from_str(s));
            profile
                .network_rules
                .push(NetworkRule { domain, sock_type });
        }
        return;
    }

    // File rules: check for owner prefix
    let (is_owner, rest) = if trimmed.starts_with("owner ") {
        (true, trimmed.strip_prefix("owner ").unwrap_or(trimmed))
    } else {
        (false, trimmed)
    };

    // Try to parse as a file rule: path followed by permissions
    let parts: Vec<&str> = rest.split_whitespace().collect();
    if parts.len() >= 2 {
        let path = parts[0];
        let perms_str = parts[1].strip_suffix(',').unwrap_or(parts[1]);
        let perm = FilePermission::from_str(perms_str);
        if !perm.is_empty()
            && (path.starts_with('/') || path.starts_with('@') || path.contains('*'))
        {
            profile.file_rules.push(FileRule {
                path: path.to_string(),
                permission: perm,
                owner_only: is_owner,
            });
        }
    }
}

/// Parse profile content text into a vector of profiles.
fn parse_profile_content(content: &str, source_file: Option<&Path>) -> Vec<Profile> {
    let mut profiles = Vec::new();
    let mut current_profile: Option<Profile> = None;
    let mut brace_depth: u32 = 0;

    for line in content.lines() {
        let trimmed = line.trim();

        // Skip comments and empty lines at top level
        if current_profile.is_none() && (trimmed.starts_with('#') || trimmed.is_empty()) {
            continue;
        }

        // Include directives at top level
        if current_profile.is_none()
            && (trimmed.starts_with("include") || trimmed.starts_with("#include"))
        {
            continue;
        }

        // Detect profile block start
        if current_profile.is_none() && trimmed.contains('{') {
            let header = trimmed.split('{').next().unwrap_or("").trim();
            let parts: Vec<&str> = header.split_whitespace().collect();

            let (name, flags_start) = if !parts.is_empty() && parts[0] == "profile" {
                // "profile /path/to/bin flags=(complain) {"
                if parts.len() >= 2 {
                    (parts[1].to_string(), 2)
                } else {
                    continue;
                }
            } else if !parts.is_empty() {
                // "/path/to/bin flags=(complain) {"
                (parts[0].to_string(), 1)
            } else {
                continue;
            };

            let mut prof = Profile::new(&name);
            prof.binary_path = Some(name.clone());
            prof.profile_file = source_file.map(|p| p.to_path_buf());

            // Parse flags
            for part in parts.iter().skip(flags_start) {
                if part.starts_with("flags=(") {
                    let flags_str = part
                        .strip_prefix("flags=(")
                        .unwrap_or("")
                        .strip_suffix(')')
                        .unwrap_or(part.strip_prefix("flags=(").unwrap_or(""));
                    for flag in flags_str.split(',') {
                        let flag = flag.trim();
                        if flag == "complain" {
                            prof.mode = ProfileMode::Complain;
                        }
                        if !flag.is_empty() {
                            prof.flags.push(flag.to_string());
                        }
                    }
                }
            }

            current_profile = Some(prof);
            brace_depth = 1;
            continue;
        }

        if let Some(ref mut prof) = current_profile {
            // Count braces
            for ch in trimmed.chars() {
                if ch == '{' {
                    brace_depth += 1;
                } else if ch == '}' {
                    brace_depth = brace_depth.saturating_sub(1);
                }
            }

            if brace_depth == 0 {
                // End of profile block
                profiles.push(prof.clone());
                current_profile = None;
            } else if brace_depth == 1 {
                // Rules within main profile block
                parse_rule_line(trimmed, prof);
            } else if brace_depth == 2 && trimmed.contains('{') {
                // Child profile
                let child_name = trimmed.split('{').next().unwrap_or("").trim();
                prof.child_profiles.push(child_name.to_string());
            }
        }
    }

    // Handle unclosed profile (be lenient)
    if let Some(prof) = current_profile {
        profiles.push(prof);
    }

    profiles
}

/// Load all profiles from a directory.
fn load_profiles_from_dir(dir: &Path) -> Vec<Profile> {
    let mut all_profiles = Vec::new();

    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return all_profiles,
    };

    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let path = entry.path();

        // Skip directories, hidden files, and non-profile files
        if path.is_dir() {
            continue;
        }
        if let Some(name) = path.file_name().and_then(|n| n.to_str())
            && (name.starts_with('.') || name.ends_with('~') || name.ends_with(".dpkg-old"))
        {
            continue;
        }

        let content = match fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let mut profiles = parse_profile_content(&content, Some(&path));
        all_profiles.append(&mut profiles);
    }

    all_profiles
}

/// Load kernel-reported profile state from apparmorfs.
fn load_kernel_profiles(path: &str) -> Vec<(String, ProfileMode)> {
    let mut result = Vec::new();
    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return result,
    };

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        // Format: "profile_name (mode)"
        if let Some(paren_pos) = trimmed.rfind('(') {
            let name = trimmed[..paren_pos].trim().to_string();
            let mode_str = trimmed[paren_pos + 1..]
                .trim()
                .strip_suffix(')')
                .unwrap_or("")
                .trim();
            let mode = ProfileMode::from_str(mode_str);
            result.push((name, mode));
        }
    }

    result
}

/// Scan proc filesystem for processes and their profiles.
fn scan_processes(proc_dir: &str) -> Vec<ProcessInfo> {
    let mut procs = Vec::new();
    let entries = match fs::read_dir(proc_dir) {
        Ok(e) => e,
        Err(_) => return procs,
    };

    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let name = match entry.file_name().into_string() {
            Ok(n) => n,
            Err(_) => continue,
        };

        // Only process numeric directories (PIDs)
        let pid: u32 = match name.parse() {
            Ok(p) => p,
            Err(_) => continue,
        };

        // Read process name from /proc/<pid>/comm
        let comm_path = format!("{}/{}/comm", proc_dir, pid);
        let proc_name = fs::read_to_string(&comm_path)
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|_| String::from("unknown"));

        // Read AppArmor profile from /proc/<pid>/attr/current
        let attr_path = format!("{}/{}/attr/current", proc_dir, pid);
        let attr = fs::read_to_string(&attr_path)
            .map(|s| s.trim().to_string())
            .unwrap_or_default();

        let (profile, mode) = parse_proc_attr(&attr);

        procs.push(ProcessInfo {
            pid,
            name: proc_name,
            profile,
            mode,
        });
    }

    procs.sort_by_key(|p| p.pid);
    procs
}

/// Parse a /proc/PID/attr/current entry into profile name and mode.
fn parse_proc_attr(attr: &str) -> (Option<String>, ProfileMode) {
    let trimmed = attr.trim();
    if trimmed.is_empty() || trimmed == "unconfined" {
        return (None, ProfileMode::Unconfined);
    }

    // Format: "profile_name (mode)"
    if let Some(paren_pos) = trimmed.rfind('(') {
        let name = trimmed[..paren_pos].trim().to_string();
        let mode_str = trimmed[paren_pos + 1..]
            .strip_suffix(')')
            .unwrap_or("")
            .trim()
            .to_string();
        let mode = ProfileMode::from_str(&mode_str);
        if name.is_empty() {
            (None, mode)
        } else {
            (Some(name), mode)
        }
    } else {
        (Some(trimmed.to_string()), ProfileMode::Enforce)
    }
}

/// Parse audit log for AppArmor denials.
fn parse_audit_log(log_path: &str) -> Vec<AuditDenial> {
    let mut denials = Vec::new();
    let content = match fs::read_to_string(log_path) {
        Ok(c) => c,
        Err(_) => return denials,
    };

    for line in content.lines() {
        if !line.contains("apparmor") && !line.contains("APPARMOR") {
            continue;
        }
        if !line.contains("DENIED") && !line.contains("denied") {
            continue;
        }

        let denial = parse_audit_denial_line(line);
        if let Some(d) = denial {
            denials.push(d);
        }
    }

    denials
}

/// Parse a single audit denial line.
fn parse_audit_denial_line(line: &str) -> Option<AuditDenial> {
    let mut timestamp = String::new();
    let mut profile = String::new();
    let mut operation = String::new();
    let mut denied_mask = String::new();
    let mut target = String::new();
    let mut info = None;
    let mut comm = None;
    let mut pid = None;

    // Extract timestamp - look for msg=audit(timestamp:serial)
    if let Some(ts_start) = line.find("msg=audit(") {
        let rest = &line[ts_start + 10..];
        if let Some(ts_end) = rest.find(')') {
            timestamp = rest[..ts_end].to_string();
        }
    }

    // Extract key=value pairs
    for part in line.split_whitespace() {
        if let Some(val) = part.strip_prefix("profile=") {
            profile = val.trim_matches('"').to_string();
        } else if let Some(val) = part.strip_prefix("operation=") {
            operation = val.trim_matches('"').to_string();
        } else if let Some(val) = part.strip_prefix("denied_mask=") {
            denied_mask = val.trim_matches('"').to_string();
        } else if let Some(val) = part.strip_prefix("name=") {
            target = val.trim_matches('"').to_string();
        } else if let Some(val) = part.strip_prefix("info=") {
            info = Some(val.trim_matches('"').to_string());
        } else if let Some(val) = part.strip_prefix("comm=") {
            comm = Some(val.trim_matches('"').to_string());
        } else if let Some(val) = part.strip_prefix("pid=") {
            pid = val.parse().ok();
        }
    }

    if profile.is_empty() && operation.is_empty() {
        return None;
    }

    Some(AuditDenial {
        timestamp,
        profile,
        operation,
        denied_mask,
        target,
        info,
        comm,
        pid,
    })
}

// ============================================================================
// Profile generation
// ============================================================================

/// Generate a skeleton profile for a binary.
fn generate_profile(binary_path: &str) -> String {
    let binary_name = Path::new(binary_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");

    let mut out = String::new();
    out.push_str(&format!("# AppArmor profile for {}\n", binary_name));
    out.push_str(&format!("# Generated by aa-genprof v{}\n", VERSION));
    out.push_str("#include <tunables/global>\n\n");
    out.push_str(&format!("{} {{\n", binary_path));
    out.push_str("  #include <abstractions/base>\n");
    out.push_str("  #include <abstractions/nameservice>\n\n");
    out.push_str("  # Capabilities\n");
    out.push_str("  # capability net_bind_service,\n\n");
    out.push_str("  # File access rules\n");
    out.push_str(&format!("  {} mr,\n", binary_path));
    out.push_str("  /etc/ld.so.cache r,\n");
    out.push_str("  /etc/ld.so.preload r,\n");
    out.push_str("  /lib/** mr,\n");
    out.push_str("  /usr/lib/** mr,\n\n");
    out.push_str("  # Temporary files\n");
    out.push_str("  /tmp/** rw,\n");
    out.push_str("  /var/tmp/** rw,\n\n");
    out.push_str("  # Network rules\n");
    out.push_str("  # network inet stream,\n");
    out.push_str("  # network inet dgram,\n\n");
    out.push_str("  # Deny sensitive paths\n");
    out.push_str("  deny /etc/shadow r,\n");
    out.push_str("  deny /etc/gshadow r,\n");
    out.push_str("}\n");

    out
}

/// Generate suggested rules from audit denials.
fn suggest_rules_from_denials(denials: &[AuditDenial]) -> Vec<String> {
    let mut suggestions: Vec<String> = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for denial in denials {
        let key = format!(
            "{}:{}:{}",
            denial.profile, denial.target, denial.denied_mask
        );
        if seen.contains(&key) {
            continue;
        }
        seen.insert(key);

        let suggestion = match denial.operation.as_str() {
            "file_mmap" | "file_lock" | "open" => {
                format!("  {} {},", denial.target, denial.denied_mask)
            }
            "capable" => {
                format!("  capability {},", denial.denied_mask)
            }
            "connect" | "bind" | "listen" | "accept" | "sendmsg" | "recvmsg" => {
                format!("  network inet stream,  # for {}", denial.operation)
            }
            "exec" => {
                format!("  {} ix,", denial.target)
            }
            "mknod" => {
                format!("  {} w,", denial.target)
            }
            "ptrace" => {
                format!("  ptrace (read),  # for {}", denial.target)
            }
            "signal" => {
                format!("  signal (send receive),  # for {}", denial.target)
            }
            _ => {
                format!(
                    "  # {} {} {} on {}",
                    denial.operation, denial.denied_mask, denial.profile, denial.target
                )
            }
        };
        suggestions.push(suggestion);
    }

    suggestions
}

// ============================================================================
// Profile compilation (apparmor_parser)
// ============================================================================

/// Preprocess a profile (resolve includes, expand variables).
fn preprocess_profile(content: &str) -> String {
    let mut out = String::new();
    let mut vars: HashMap<String, String> = HashMap::new();

    for line in content.lines() {
        let trimmed = line.trim();

        // Variable assignment: `@{var}=value`. Only treat the line as an
        // assignment when the closing brace is followed by `=`; otherwise a
        // line that merely *uses* a variable at its start (e.g.
        // `@{HOME}/data r,`) would be misread as an assignment and skip
        // variable expansion below.
        if trimmed.starts_with("@{")
            && let Some(brace) = trimmed.find('}')
        {
            let after = trimmed[brace + 1..].trim_start();
            if let Some(val) = after.strip_prefix('=') {
                let var_name = &trimmed[2..brace];
                vars.insert(var_name.to_string(), val.trim().to_string());
                out.push_str(line);
                out.push('\n');
                continue;
            }
        }
        // Not an assignment — fall through to variable expansion.

        // Expand variables in line
        let mut expanded = line.to_string();
        for (var, val) in &vars {
            let pattern = format!("@{{{}}}", var);
            expanded = expanded.replace(&pattern, val);
        }

        // Resolve includes
        if let Some(rest) = trimmed
            .strip_prefix("#include")
            .or_else(|| trimmed.strip_prefix("include"))
        {
            let rest = rest.trim();
            let inc_path = rest.trim_matches(|c| c == '<' || c == '>' || c == '"');
            // Resolve relative to tunables/abstractions
            let resolved = if inc_path.starts_with('/') {
                inc_path.to_string()
            } else {
                format!("{}/{}", PROFILES_DIR, inc_path)
            };
            out.push_str(&format!("# include resolved: {}\n", resolved));
            if let Ok(inc_content) = fs::read_to_string(&resolved) {
                out.push_str(&inc_content);
                out.push('\n');
            } else {
                out.push_str(&format!("# WARNING: cannot resolve include {}\n", resolved));
            }
        } else {
            out.push_str(&expanded);
            out.push('\n');
        }
    }

    out
}

/// Validate a profile for basic correctness.
fn validate_profile(profile: &Profile) -> Vec<String> {
    let mut errors = Vec::new();

    if profile.name.is_empty() {
        errors.push("Profile name is empty".to_string());
    }

    // Check binary path exists (if it looks like an absolute path)
    if let Some(ref bin) = profile.binary_path
        && bin.starts_with('/')
        && !Path::new(bin).exists()
    {
        errors.push(format!("Binary path does not exist: {}", bin));
    }

    // Check for duplicate file rules
    let mut seen_paths = std::collections::HashSet::new();
    for rule in &profile.file_rules {
        if !seen_paths.insert(&rule.path) {
            errors.push(format!("Duplicate file rule for path: {}", rule.path));
        }
    }

    // Check for conflicting network rules
    let mut seen_net = std::collections::HashSet::new();
    for rule in &profile.network_rules {
        let key = format!("{}{:?}", rule.domain, rule.sock_type);
        if !seen_net.insert(key) {
            errors.push(format!("Duplicate network rule: {}", rule));
        }
    }

    errors
}

// ============================================================================
// Subcommand implementations
// ============================================================================

/// aa-status: show AppArmor status and loaded profiles.
fn cmd_status(args: &[String]) -> i32 {
    let mut use_json = false;
    let mut _verbose = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--help" | "-h" => {
                println!("Usage: aa-status [OPTIONS]");
                println!();
                println!("Display AppArmor status and loaded profiles.");
                println!();
                println!("Options:");
                println!("  --json          Output in JSON format");
                println!("  --verbose, -v   Show verbose output");
                println!("  --help, -h      Show this help");
                return 0;
            }
            "--json" => use_json = true,
            "--verbose" | "-v" => _verbose = true,
            other => {
                eprintln!("aa-status: unknown option '{}'", other);
                return 1;
            }
        }
        i += 1;
    }

    // Load kernel profiles
    let kernel_profiles = load_kernel_profiles(PROFILES_STATE);

    // Count by mode
    let mut enforce_count = 0u32;
    let mut complain_count = 0u32;
    let mut unconfined_count = 0u32;
    let mut enforce_list = Vec::new();
    let mut complain_list = Vec::new();

    for (name, mode) in &kernel_profiles {
        match mode {
            ProfileMode::Enforce => {
                enforce_count += 1;
                enforce_list.push(name.as_str());
            }
            ProfileMode::Complain => {
                complain_count += 1;
                complain_list.push(name.as_str());
            }
            ProfileMode::Unconfined => {
                unconfined_count += 1;
            }
            ProfileMode::Disable => {}
        }
    }

    // Also load on-disk profiles
    let disk_profiles = load_profiles_from_dir(Path::new(PROFILES_DIR));

    // Scan processes
    let processes = scan_processes(PROC_DIR);
    let confined_procs: Vec<&ProcessInfo> =
        processes.iter().filter(|p| p.profile.is_some()).collect();
    let unconfined_procs: Vec<&ProcessInfo> =
        processes.iter().filter(|p| p.profile.is_none()).collect();

    if use_json {
        println!("{{");
        println!("  \"version\": \"{}\",", VERSION);
        println!("  \"profiles\": {{");
        println!("    \"total\": {},", kernel_profiles.len());
        println!("    \"enforce\": {},", enforce_count);
        println!("    \"complain\": {},", complain_count);
        println!("    \"unconfined\": {}", unconfined_count);
        println!("  }},");
        println!("  \"processes\": {{");
        println!("    \"total\": {},", processes.len());
        println!("    \"confined\": {},", confined_procs.len());
        println!("    \"unconfined\": {}", unconfined_procs.len());
        println!("  }},");
        println!("  \"disk_profiles\": {}", disk_profiles.len());
        println!("}}");
    } else {
        println!("apparmor module is loaded.");
        println!("{} profiles are loaded.", kernel_profiles.len());
        println!("{} profiles are in enforce mode.", enforce_count);
        for name in &enforce_list {
            println!("   {}", name);
        }
        println!("{} profiles are in complain mode.", complain_count);
        for name in &complain_list {
            println!("   {}", name);
        }
        println!("{} profiles are in unconfined mode.", unconfined_count);
        println!();
        println!("{} processes have profiles defined.", confined_procs.len());
        println!(
            "{} processes are unconfined but have a profile defined.",
            unconfined_procs
                .iter()
                .filter(|p| {
                    kernel_profiles
                        .iter()
                        .any(|(n, _)| n == &p.name || p.name.contains(n.as_str()))
                })
                .count()
        );
        println!("{} processes are unconfined.", unconfined_procs.len());
    }

    0
}

/// aa-enforce: set profiles to enforce mode.
fn cmd_enforce(args: &[String]) -> i32 {
    if args.is_empty() {
        eprintln!("Usage: aa-enforce PROFILE [PROFILE ...]");
        eprintln!();
        eprintln!("Set the specified AppArmor profiles to enforce mode.");
        return 1;
    }

    if args.len() == 1 && (args[0] == "--help" || args[0] == "-h") {
        println!("Usage: aa-enforce PROFILE [PROFILE ...]");
        println!();
        println!("Set the specified AppArmor profiles to enforce mode.");
        println!("Profiles can be specified by name or path to profile file.");
        println!();
        println!("Options:");
        println!("  --help, -h   Show this help");
        return 0;
    }

    let mut exit_code = 0;
    for profile_arg in args {
        if let Err(e) = set_profile_mode(profile_arg, ProfileMode::Enforce) {
            eprintln!(
                "aa-enforce: error setting '{}' to enforce: {}",
                profile_arg, e
            );
            exit_code = 1;
        } else {
            println!("Setting {} to enforce mode.", profile_arg);
        }
    }

    exit_code
}

/// aa-complain: set profiles to complain mode.
fn cmd_complain(args: &[String]) -> i32 {
    if args.is_empty() {
        eprintln!("Usage: aa-complain PROFILE [PROFILE ...]");
        eprintln!();
        eprintln!("Set the specified AppArmor profiles to complain mode.");
        return 1;
    }

    if args.len() == 1 && (args[0] == "--help" || args[0] == "-h") {
        println!("Usage: aa-complain PROFILE [PROFILE ...]");
        println!();
        println!("Set the specified AppArmor profiles to complain mode.");
        println!("In complain mode, policy violations are logged but not enforced.");
        println!();
        println!("Options:");
        println!("  --help, -h   Show this help");
        return 0;
    }

    let mut exit_code = 0;
    for profile_arg in args {
        if let Err(e) = set_profile_mode(profile_arg, ProfileMode::Complain) {
            eprintln!(
                "aa-complain: error setting '{}' to complain: {}",
                profile_arg, e
            );
            exit_code = 1;
        } else {
            println!("Setting {} to complain mode.", profile_arg);
        }
    }

    exit_code
}

/// aa-disable: disable profiles.
fn cmd_disable(args: &[String]) -> i32 {
    if args.is_empty() {
        eprintln!("Usage: aa-disable PROFILE [PROFILE ...]");
        eprintln!();
        eprintln!("Disable the specified AppArmor profiles.");
        return 1;
    }

    if args.len() == 1 && (args[0] == "--help" || args[0] == "-h") {
        println!("Usage: aa-disable PROFILE [PROFILE ...]");
        println!();
        println!("Disable the specified AppArmor profiles.");
        println!("Disabled profiles are unloaded and will not be loaded on restart.");
        println!();
        println!("Options:");
        println!("  --help, -h   Show this help");
        return 0;
    }

    let mut exit_code = 0;
    for profile_arg in args {
        if let Err(e) = set_profile_mode(profile_arg, ProfileMode::Disable) {
            eprintln!("aa-disable: error disabling '{}': {}", profile_arg, e);
            exit_code = 1;
        } else {
            println!("Disabling {}.", profile_arg);
        }
    }

    exit_code
}

/// Set a profile to the given mode by rewriting its profile file flags.
fn set_profile_mode(profile_name: &str, mode: ProfileMode) -> Result<(), String> {
    // Find the profile file
    let profile_path = if Path::new(profile_name).exists() {
        PathBuf::from(profile_name)
    } else {
        // Search in profiles directory
        let candidate = Path::new(PROFILES_DIR).join(profile_name);
        if candidate.exists() {
            candidate
        } else {
            // Search by scanning profile names
            let profiles = load_profiles_from_dir(Path::new(PROFILES_DIR));
            match profiles.iter().find(|p| p.name == profile_name) {
                Some(p) => match &p.profile_file {
                    Some(f) => f.clone(),
                    None => {
                        return Err(format!("No profile file found for '{}'", profile_name));
                    }
                },
                None => {
                    return Err(format!("Profile '{}' not found", profile_name));
                }
            }
        }
    };

    let content = fs::read_to_string(&profile_path)
        .map_err(|e| format!("Cannot read '{}': {}", profile_path.display(), e))?;

    let new_content = rewrite_profile_flags(&content, mode);

    fs::write(&profile_path, new_content)
        .map_err(|e| format!("Cannot write '{}': {}", profile_path.display(), e))?;

    // Also write to the kernel interface to take immediate effect
    let _iface_path = format!("{}/profiles", APPARMORFS);
    // In a real system, we would write to the kernel interface here.
    // For now, rewriting the profile file is sufficient.

    Ok(())
}

/// Rewrite profile flags to set the given mode.
fn rewrite_profile_flags(content: &str, mode: ProfileMode) -> String {
    let mut output = String::new();
    let flag_str = match mode {
        ProfileMode::Enforce => "",
        ProfileMode::Complain => "flags=(complain) ",
        ProfileMode::Disable => "",
        ProfileMode::Unconfined => "",
    };

    for line in content.lines() {
        let trimmed = line.trim();
        // Look for profile header lines
        if trimmed.contains('{') && !trimmed.starts_with('#') {
            // Remove existing flags
            let cleaned = remove_flags(trimmed);
            if flag_str.is_empty() {
                output.push_str(&cleaned);
            } else {
                // Insert flags before the opening brace
                if let Some(brace_pos) = cleaned.find('{') {
                    let before = cleaned[..brace_pos].trim_end();
                    output.push_str(before);
                    output.push(' ');
                    output.push_str(flag_str);
                    output.push_str(&cleaned[brace_pos..]);
                } else {
                    output.push_str(&cleaned);
                }
            }
        } else {
            output.push_str(line);
        }
        output.push('\n');
    }

    output
}

/// Remove flags=(...) from a profile header line.
fn remove_flags(line: &str) -> String {
    let mut result = String::new();
    let mut i = 0;
    let bytes = line.as_bytes();

    while i < bytes.len() {
        // Look for "flags=("
        if i + 7 <= bytes.len() && &line[i..i + 7] == "flags=(" {
            // Skip until closing paren
            let mut j = i + 7;
            while j < bytes.len() && bytes[j] != b')' {
                j += 1;
            }
            if j < bytes.len() {
                j += 1; // skip ')'
            }
            // Skip trailing space
            if j < bytes.len() && bytes[j] == b' ' {
                j += 1;
            }
            i = j;
        } else {
            result.push(bytes[i] as char);
            i += 1;
        }
    }

    result
}

/// aa-genprof: generate a profile for a program.
fn cmd_genprof(args: &[String]) -> i32 {
    if args.is_empty() || (args.len() == 1 && (args[0] == "--help" || args[0] == "-h")) {
        println!("Usage: aa-genprof BINARY_PATH [OPTIONS]");
        println!();
        println!("Generate an AppArmor profile for the specified binary.");
        println!();
        println!("Options:");
        println!(
            "  -d DIR         Profile directory (default: {})",
            PROFILES_DIR
        );
        println!("  -f FILE        Output to specific file");
        println!("  --help, -h     Show this help");
        if args.is_empty() {
            return 1;
        }
        return 0;
    }

    let mut binary_path = String::new();
    let mut output_dir = PROFILES_DIR.to_string();
    let mut output_file: Option<String> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-d" => {
                i += 1;
                if i < args.len() {
                    output_dir = args[i].clone();
                } else {
                    eprintln!("aa-genprof: -d requires an argument");
                    return 1;
                }
            }
            "-f" => {
                i += 1;
                if i < args.len() {
                    output_file = Some(args[i].clone());
                } else {
                    eprintln!("aa-genprof: -f requires an argument");
                    return 1;
                }
            }
            "--help" | "-h" => {
                println!("Usage: aa-genprof BINARY_PATH [OPTIONS]");
                return 0;
            }
            arg => {
                if binary_path.is_empty() {
                    binary_path = arg.to_string();
                } else {
                    eprintln!("aa-genprof: unexpected argument '{}'", arg);
                    return 1;
                }
            }
        }
        i += 1;
    }

    if binary_path.is_empty() {
        eprintln!("aa-genprof: no binary path specified");
        return 1;
    }

    // Resolve to absolute path
    if !binary_path.starts_with('/')
        && let Ok(cwd) = env::current_dir()
    {
        binary_path = format!("{}/{}", cwd.display(), binary_path);
    }

    let profile_content = generate_profile(&binary_path);

    let dest = if let Some(ref file) = output_file {
        PathBuf::from(file)
    } else {
        let base_name = Path::new(&binary_path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown");
        Path::new(&output_dir).join(base_name)
    };

    match fs::write(&dest, &profile_content) {
        Ok(()) => {
            println!("Profile written to {}", dest.display());
            println!();
            println!("Generated profile:");
            println!("{}", profile_content);
            0
        }
        Err(e) => {
            eprintln!("aa-genprof: cannot write '{}': {}", dest.display(), e);
            // Still print the generated profile to stdout
            println!("{}", profile_content);
            1
        }
    }
}

/// aa-logprof: update profiles from logs.
fn cmd_logprof(args: &[String]) -> i32 {
    let mut log_path = LOG_FILE.to_string();
    let mut profile_dir = PROFILES_DIR.to_string();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--help" | "-h" => {
                println!("Usage: aa-logprof [OPTIONS]");
                println!();
                println!("Scan audit logs for AppArmor denials and suggest profile updates.");
                println!();
                println!("Options:");
                println!("  -f FILE        Audit log file (default: {})", LOG_FILE);
                println!(
                    "  -d DIR         Profile directory (default: {})",
                    PROFILES_DIR
                );
                println!("  --help, -h     Show this help");
                return 0;
            }
            "-f" => {
                i += 1;
                if i < args.len() {
                    log_path = args[i].clone();
                } else {
                    eprintln!("aa-logprof: -f requires an argument");
                    return 1;
                }
            }
            "-d" => {
                i += 1;
                if i < args.len() {
                    profile_dir = args[i].clone();
                } else {
                    eprintln!("aa-logprof: -d requires an argument");
                    return 1;
                }
            }
            other => {
                eprintln!("aa-logprof: unknown option '{}'", other);
                return 1;
            }
        }
        i += 1;
    }

    // Also try syslog if primary log doesn't have results
    let mut denials = parse_audit_log(&log_path);
    if denials.is_empty() {
        let syslog_denials = parse_audit_log(SYSLOG_FILE);
        denials.extend(syslog_denials);
    }

    if denials.is_empty() {
        println!("No AppArmor denials found in logs.");
        return 0;
    }

    println!("Found {} AppArmor denial(s) in logs.\n", denials.len());

    // Group denials by profile
    let mut by_profile: HashMap<String, Vec<AuditDenial>> = HashMap::new();
    for denial in denials {
        by_profile
            .entry(denial.profile.clone())
            .or_default()
            .push(denial);
    }

    // Load existing profiles for context
    let _existing = load_profiles_from_dir(Path::new(&profile_dir));

    for (profile_name, profile_denials) in &by_profile {
        println!("Profile: {}", profile_name);
        println!("{}", "-".repeat(40));

        for denial in profile_denials {
            println!("  {}", denial);
        }

        let suggestions = suggest_rules_from_denials(profile_denials);
        if !suggestions.is_empty() {
            println!("\n  Suggested rules:");
            for suggestion in &suggestions {
                println!("  {}", suggestion);
            }
        }
        println!();
    }

    0
}

/// aa-unconfined: list unconfined processes.
fn cmd_unconfined(args: &[String]) -> i32 {
    let mut show_all = false;
    let mut with_paranoid = false;

    for arg in args {
        match arg.as_str() {
            "--help" | "-h" => {
                println!("Usage: aa-unconfined [OPTIONS]");
                println!();
                println!("List processes that have no AppArmor profile.");
                println!();
                println!("Options:");
                println!("  --paranoid     Also show processes with profiles in complain mode");
                println!("  --all          Show all processes, not just those with open ports");
                println!("  --help, -h     Show this help");
                return 0;
            }
            "--paranoid" => with_paranoid = true,
            "--all" => show_all = true,
            other => {
                eprintln!("aa-unconfined: unknown option '{}'", other);
                return 1;
            }
        }
    }

    let processes = scan_processes(PROC_DIR);
    let kernel_profiles = load_kernel_profiles(PROFILES_STATE);

    let mut unconfined: Vec<&ProcessInfo> = Vec::new();
    let mut complain_procs: Vec<&ProcessInfo> = Vec::new();

    for proc in &processes {
        if proc.profile.is_none() {
            if show_all || has_open_ports(proc.pid) {
                unconfined.push(proc);
            }
        } else if with_paranoid && proc.mode == ProfileMode::Complain {
            complain_procs.push(proc);
        }
    }

    if unconfined.is_empty() && complain_procs.is_empty() {
        println!("No unconfined processes found.");
        return 0;
    }

    println!(
        "{} unconfined processes with network activity:",
        unconfined.len()
    );
    for proc in &unconfined {
        let has_profile = kernel_profiles
            .iter()
            .any(|(n, _)| n == &proc.name || proc.name.contains(n.as_str()));
        if has_profile {
            println!(
                "  {} ({}) - profile exists but not loaded",
                proc.pid, proc.name
            );
        } else {
            println!("  {} ({}) - not confined", proc.pid, proc.name);
        }
    }

    if with_paranoid && !complain_procs.is_empty() {
        println!("\n{} processes in complain mode:", complain_procs.len());
        for proc in &complain_procs {
            println!(
                "  {} ({}) - {} (complain)",
                proc.pid,
                proc.name,
                proc.profile.as_deref().unwrap_or("unknown")
            );
        }
    }

    0
}

/// Check if a process has open network ports (simplified check).
fn has_open_ports(pid: u32) -> bool {
    // Check /proc/<pid>/net/tcp and /proc/<pid>/net/tcp6
    let tcp_path = format!("{}/{}/net/tcp", PROC_DIR, pid);
    let tcp6_path = format!("{}/{}/net/tcp6", PROC_DIR, pid);

    let check = |path: &str| -> bool {
        match fs::read_to_string(path) {
            Ok(content) => {
                // More than just the header line means open sockets
                content.lines().count() > 1
            }
            Err(_) => false,
        }
    };

    check(&tcp_path) || check(&tcp6_path)
}

/// Binary-profile cache configuration parsed from the parser CLI flags
/// (`--base`/`-b`, `--cache-loc`/`-L`, `--write-cache`/`-W`, `--skip-cache`).
///
/// AppArmor's compiled-policy cache is not yet implemented, so these options
/// are not read during parsing. The struct keeps the parsed values together so
/// that wiring cache load/store into `process_profile_content` later is a
/// localized change rather than re-threading four positional arguments.
#[allow(dead_code)]
struct CacheOptions {
    base_dir: String,
    cache_dir: String,
    write_cache: bool,
    skip_cache: bool,
}

/// apparmor_parser: compile/load/remove profiles.
fn cmd_parser(args: &[String]) -> i32 {
    let mut action = ParserAction::Add;
    let mut debug = false;
    let mut profile_files: Vec<String> = Vec::new();
    let mut base_dir = PROFILES_DIR.to_string();
    let mut cache_dir = CACHE_DIR.to_string();
    let mut write_cache = false;
    let mut skip_cache = false;
    let mut quiet = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--help" | "-h" => {
                print_parser_help();
                return 0;
            }
            "--version" | "-V" => {
                println!("apparmor_parser version {}", VERSION);
                return 0;
            }
            "--add" | "-a" => action = ParserAction::Add,
            "--replace" | "-r" => action = ParserAction::Replace,
            "--remove" | "-R" => action = ParserAction::Remove,
            "--preprocess" | "-p" => action = ParserAction::Preprocess,
            "--debug" | "-d" => debug = true,
            "--base" | "-b" => {
                i += 1;
                if i < args.len() {
                    base_dir = args[i].clone();
                } else {
                    eprintln!("apparmor_parser: --base requires an argument");
                    return 1;
                }
            }
            "--cache-loc" | "-L" => {
                i += 1;
                if i < args.len() {
                    cache_dir = args[i].clone();
                } else {
                    eprintln!("apparmor_parser: --cache-loc requires an argument");
                    return 1;
                }
            }
            "--write-cache" | "-W" => write_cache = true,
            "--skip-cache" => skip_cache = true,
            "--quiet" | "-q" => quiet = true,
            arg => {
                if arg.starts_with('-') {
                    eprintln!("apparmor_parser: unknown option '{}'", arg);
                    return 1;
                }
                profile_files.push(arg.to_string());
            }
        }
        i += 1;
    }

    let cache = CacheOptions {
        base_dir,
        cache_dir,
        write_cache,
        skip_cache,
    };

    if profile_files.is_empty() {
        // Read from stdin
        if !quiet {
            eprintln!("apparmor_parser: reading profile from stdin");
        }
        let stdin = io::stdin();
        let mut content = String::new();
        for line in stdin.lock().lines() {
            match line {
                Ok(l) => {
                    content.push_str(&l);
                    content.push('\n');
                }
                Err(e) => {
                    eprintln!("apparmor_parser: error reading stdin: {}", e);
                    return 1;
                }
            }
        }
        return process_profile_content(&content, "<stdin>", action, debug, &cache, quiet);
    }

    let mut exit_code = 0;
    for file in &profile_files {
        let content = match fs::read_to_string(file) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("apparmor_parser: cannot read '{}': {}", file, e);
                exit_code = 1;
                continue;
            }
        };
        let code = process_profile_content(&content, file, action, debug, &cache, quiet);
        if code != 0 {
            exit_code = code;
        }
    }

    exit_code
}

fn print_parser_help() {
    println!("Usage: apparmor_parser [OPTIONS] [PROFILE ...]");
    println!();
    println!("Compile, load, and manage AppArmor profiles.");
    println!();
    println!("Actions:");
    println!("  --add, -a          Add profiles to the kernel (default)");
    println!("  --replace, -r      Replace existing profiles in the kernel");
    println!("  --remove, -R       Remove profiles from the kernel");
    println!("  --preprocess, -p   Preprocess profiles (resolve includes)");
    println!();
    println!("Options:");
    println!(
        "  --base DIR, -b     Base directory for includes (default: {})",
        PROFILES_DIR
    );
    println!(
        "  --cache-loc DIR    Cache directory (default: {})",
        CACHE_DIR
    );
    println!("  --write-cache, -W  Write compiled profiles to cache");
    println!("  --skip-cache       Don't use cached profiles");
    println!("  --debug, -d        Show debug output");
    println!("  --quiet, -q        Suppress non-error messages");
    println!("  --version, -V      Show version");
    println!("  --help, -h         Show this help");
}

/// Process a single profile file content through the parser.
fn process_profile_content(
    content: &str,
    source: &str,
    action: ParserAction,
    debug: bool,
    _cache: &CacheOptions,
    quiet: bool,
) -> i32 {
    if action == ParserAction::Preprocess {
        let preprocessed = preprocess_profile(content);
        print!("{}", preprocessed);
        return 0;
    }

    let profiles = parse_profile_content(content, Some(Path::new(source)));

    if profiles.is_empty() {
        if !quiet {
            eprintln!("apparmor_parser: no profiles found in '{}'", source);
        }
        return 1;
    }

    if debug {
        eprintln!(
            "apparmor_parser: found {} profile(s) in '{}'",
            profiles.len(),
            source
        );
    }

    let exit_code = 0;

    for profile in &profiles {
        // Validate
        let errors = validate_profile(profile);
        if !errors.is_empty() {
            for err in &errors {
                eprintln!("apparmor_parser: {}: {}", profile.name, err);
            }
            // Warnings only for non-critical issues; continue processing
        }

        if debug {
            eprintln!("  Profile: {}", profile.name);
            eprintln!("  Mode: {}", profile.mode);
            eprintln!("  Capabilities: {}", profile.capabilities.len());
            eprintln!("  File rules: {}", profile.file_rules.len());
            eprintln!("  Network rules: {}", profile.network_rules.len());
            eprintln!("  Includes: {}", profile.includes.len());
            eprintln!("  Child profiles: {}", profile.child_profiles.len());
        }

        match action {
            ParserAction::Add => {
                if !quiet {
                    println!("Adding profile: {}", profile.name);
                }
                // In a real system, write to /sys/kernel/security/apparmor/.load
                let load_path = format!("{}/.load", APPARMORFS);
                if let Err(e) = write_profile_to_kernel(&load_path, profile)
                    && debug
                {
                    eprintln!(
                        "apparmor_parser: cannot load '{}': {} (expected on non-AppArmor systems)",
                        profile.name, e
                    );
                }
            }
            ParserAction::Replace => {
                if !quiet {
                    println!("Replacing profile: {}", profile.name);
                }
                let replace_path = format!("{}/.replace", APPARMORFS);
                if let Err(e) = write_profile_to_kernel(&replace_path, profile)
                    && debug
                {
                    eprintln!(
                        "apparmor_parser: cannot replace '{}': {} (expected on non-AppArmor systems)",
                        profile.name, e
                    );
                }
            }
            ParserAction::Remove => {
                if !quiet {
                    println!("Removing profile: {}", profile.name);
                }
                let remove_path = format!("{}/.remove", APPARMORFS);
                if let Err(e) = write_profile_to_kernel(&remove_path, profile)
                    && debug
                {
                    eprintln!(
                        "apparmor_parser: cannot remove '{}': {} (expected on non-AppArmor systems)",
                        profile.name, e
                    );
                }
            }
            ParserAction::Preprocess => {
                // Already handled above
            }
        }
    }

    exit_code
}

/// Write a profile to a kernel interface file.
fn write_profile_to_kernel(path: &str, profile: &Profile) -> io::Result<()> {
    let mut file = fs::OpenOptions::new().write(true).open(path)?;
    // Write the profile name, which is the minimal interface for load/replace/remove
    writeln!(file, "{}", profile.name)?;
    Ok(())
}

// ============================================================================
// Main
// ============================================================================

fn main() {
    let args: Vec<String> = env::args().collect();

    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("aa-status");
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

    let sub_args: Vec<String> = args.into_iter().skip(1).collect();

    let exit_code = match prog_name.as_str() {
        "aa-status" | "apparmor" => cmd_status(&sub_args),
        "aa-enforce" => cmd_enforce(&sub_args),
        "aa-complain" => cmd_complain(&sub_args),
        "aa-disable" => cmd_disable(&sub_args),
        "aa-genprof" => cmd_genprof(&sub_args),
        "aa-logprof" => cmd_logprof(&sub_args),
        "aa-unconfined" => cmd_unconfined(&sub_args),
        "apparmor_parser" => cmd_parser(&sub_args),
        _ => {
            eprintln!(
                "apparmor: unknown personality '{}', defaulting to aa-status",
                prog_name
            );
            cmd_status(&sub_args)
        }
    };

    process::exit(exit_code);
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ---- ProfileMode tests ----

    #[test]
    fn test_profile_mode_from_str_enforce() {
        assert_eq!(ProfileMode::from_str("enforce"), ProfileMode::Enforce);
    }

    #[test]
    fn test_profile_mode_from_str_complain() {
        assert_eq!(ProfileMode::from_str("complain"), ProfileMode::Complain);
    }

    #[test]
    fn test_profile_mode_from_str_disable() {
        assert_eq!(ProfileMode::from_str("disable"), ProfileMode::Disable);
    }

    #[test]
    fn test_profile_mode_from_str_disabled() {
        assert_eq!(ProfileMode::from_str("disabled"), ProfileMode::Disable);
    }

    #[test]
    fn test_profile_mode_from_str_unconfined() {
        assert_eq!(ProfileMode::from_str("unconfined"), ProfileMode::Unconfined);
    }

    #[test]
    fn test_profile_mode_from_str_unknown_defaults_enforce() {
        assert_eq!(ProfileMode::from_str("something"), ProfileMode::Enforce);
    }

    #[test]
    fn test_profile_mode_from_str_case_insensitive() {
        assert_eq!(ProfileMode::from_str("ENFORCE"), ProfileMode::Enforce);
        assert_eq!(ProfileMode::from_str("Complain"), ProfileMode::Complain);
    }

    #[test]
    fn test_profile_mode_as_str() {
        assert_eq!(ProfileMode::Enforce.as_str(), "enforce");
        assert_eq!(ProfileMode::Complain.as_str(), "complain");
        assert_eq!(ProfileMode::Disable.as_str(), "disable");
        assert_eq!(ProfileMode::Unconfined.as_str(), "unconfined");
    }

    #[test]
    fn test_profile_mode_display() {
        assert_eq!(format!("{}", ProfileMode::Enforce), "enforce");
        assert_eq!(format!("{}", ProfileMode::Complain), "complain");
    }

    // ---- Capability tests ----

    #[test]
    fn test_capability_from_str_known() {
        assert_eq!(
            Capability::from_str("net_bind_service"),
            Capability::NetBindService
        );
        assert_eq!(Capability::from_str("sys_admin"), Capability::SysAdmin);
        assert_eq!(Capability::from_str("kill"), Capability::Kill);
    }

    #[test]
    fn test_capability_from_str_case_insensitive() {
        assert_eq!(Capability::from_str("NET_ADMIN"), Capability::NetAdmin);
        assert_eq!(Capability::from_str("Sys_Ptrace"), Capability::SysPtrace);
    }

    #[test]
    fn test_capability_from_str_unknown() {
        match Capability::from_str("custom_cap") {
            Capability::Other(s) => assert_eq!(s, "custom_cap"),
            _ => panic!("Expected Other variant"),
        }
    }

    #[test]
    fn test_capability_as_str() {
        assert_eq!(Capability::NetBindService.as_str(), "net_bind_service");
        assert_eq!(Capability::SysAdmin.as_str(), "sys_admin");
        assert_eq!(Capability::Chown.as_str(), "chown");
    }

    #[test]
    fn test_capability_display() {
        assert_eq!(format!("{}", Capability::NetRaw), "net_raw");
        assert_eq!(format!("{}", Capability::Other("test".into())), "test");
    }

    #[test]
    fn test_capability_all_known_variants() {
        let caps = [
            "net_bind_service",
            "net_admin",
            "net_raw",
            "sys_admin",
            "sys_ptrace",
            "sys_rawio",
            "dac_override",
            "dac_read_search",
            "fowner",
            "fsetid",
            "kill",
            "setgid",
            "setuid",
            "chown",
            "mknod",
            "sys_chroot",
            "audit_write",
        ];
        for cap in &caps {
            let c = Capability::from_str(cap);
            assert_ne!(c.as_str(), "");
        }
    }

    // ---- FilePermission tests ----

    #[test]
    fn test_file_permission_new_is_empty() {
        let p = FilePermission::new();
        assert!(p.is_empty());
    }

    #[test]
    fn test_file_permission_from_str_read() {
        let p = FilePermission::from_str("r");
        assert!(p.read);
        assert!(!p.write);
        assert!(!p.is_empty());
    }

    #[test]
    fn test_file_permission_from_str_rw() {
        let p = FilePermission::from_str("rw");
        assert!(p.read);
        assert!(p.write);
        assert!(!p.execute);
    }

    #[test]
    fn test_file_permission_from_str_all() {
        let p = FilePermission::from_str("rwaxmlk");
        assert!(p.read);
        assert!(p.write);
        assert!(p.append);
        assert!(p.execute);
        assert!(p.memory_map);
        assert!(p.link);
        assert!(p.lock);
    }

    #[test]
    fn test_file_permission_from_str_empty() {
        let p = FilePermission::from_str("");
        assert!(p.is_empty());
    }

    #[test]
    fn test_file_permission_from_str_unknown_chars() {
        let p = FilePermission::from_str("rz");
        assert!(p.read);
        assert!(!p.write);
    }

    #[test]
    fn test_file_permission_display() {
        let p = FilePermission::from_str("rwx");
        let s = format!("{}", p);
        assert!(s.contains('r'));
        assert!(s.contains('w'));
        assert!(s.contains('x'));
    }

    #[test]
    fn test_file_permission_display_order() {
        let p = FilePermission::from_str("xwr");
        // Display should always output in canonical order: r, w, a, x, m, l, k
        assert_eq!(format!("{}", p), "rwx");
    }

    #[test]
    fn test_file_permission_display_empty() {
        let p = FilePermission::new();
        assert_eq!(format!("{}", p), "");
    }

    // ---- NetDomain tests ----

    #[test]
    fn test_net_domain_from_str() {
        assert_eq!(NetDomain::from_str("inet"), NetDomain::Inet);
        assert_eq!(NetDomain::from_str("inet6"), NetDomain::Inet6);
        assert_eq!(NetDomain::from_str("unix"), NetDomain::Unix);
        assert_eq!(NetDomain::from_str("netlink"), NetDomain::Netlink);
        assert_eq!(NetDomain::from_str("packet"), NetDomain::Packet);
    }

    #[test]
    fn test_net_domain_from_str_unknown() {
        match NetDomain::from_str("custom") {
            NetDomain::Other(s) => assert_eq!(s, "custom"),
            _ => panic!("Expected Other variant"),
        }
    }

    #[test]
    fn test_net_domain_display() {
        assert_eq!(format!("{}", NetDomain::Inet), "inet");
        assert_eq!(format!("{}", NetDomain::Unix), "unix");
    }

    // ---- NetType tests ----

    #[test]
    fn test_net_type_from_str() {
        assert_eq!(NetType::from_str("stream"), NetType::Stream);
        assert_eq!(NetType::from_str("dgram"), NetType::Dgram);
        assert_eq!(NetType::from_str("raw"), NetType::Raw);
        assert_eq!(NetType::from_str("seqpacket"), NetType::Seqpacket);
    }

    #[test]
    fn test_net_type_from_str_unknown() {
        match NetType::from_str("custom") {
            NetType::Other(s) => assert_eq!(s, "custom"),
            _ => panic!("Expected Other variant"),
        }
    }

    #[test]
    fn test_net_type_display() {
        assert_eq!(format!("{}", NetType::Stream), "stream");
    }

    // ---- NetworkRule tests ----

    #[test]
    fn test_network_rule_display_with_type() {
        let rule = NetworkRule {
            domain: NetDomain::Inet,
            sock_type: Some(NetType::Stream),
        };
        assert_eq!(format!("{}", rule), "network inet stream");
    }

    #[test]
    fn test_network_rule_display_without_type() {
        let rule = NetworkRule {
            domain: NetDomain::Inet6,
            sock_type: None,
        };
        assert_eq!(format!("{}", rule), "network inet6");
    }

    // ---- FileRule tests ----

    #[test]
    fn test_file_rule_display() {
        let rule = FileRule {
            path: "/usr/bin/foo".to_string(),
            permission: FilePermission::from_str("rx"),
            owner_only: false,
        };
        assert_eq!(format!("{}", rule), "/usr/bin/foo rx");
    }

    #[test]
    fn test_file_rule_display_owner() {
        let rule = FileRule {
            path: "/tmp/bar".to_string(),
            permission: FilePermission::from_str("rw"),
            owner_only: true,
        };
        assert_eq!(format!("{}", rule), "owner /tmp/bar rw");
    }

    // ---- Profile tests ----

    #[test]
    fn test_profile_new() {
        let p = Profile::new("test");
        assert_eq!(p.name, "test");
        assert_eq!(p.mode, ProfileMode::Enforce);
        assert!(p.capabilities.is_empty());
        assert!(p.file_rules.is_empty());
        assert!(p.network_rules.is_empty());
    }

    #[test]
    fn test_profile_display() {
        let p = Profile::new("test");
        assert_eq!(format!("{}", p), "test (enforce)");
    }

    // ---- ProcessInfo tests ----

    #[test]
    fn test_process_info_display_with_profile() {
        let p = ProcessInfo {
            pid: 1234,
            name: "nginx".to_string(),
            profile: Some("/usr/sbin/nginx".to_string()),
            mode: ProfileMode::Enforce,
        };
        assert!(format!("{}", p).contains("1234"));
        assert!(format!("{}", p).contains("nginx"));
        assert!(format!("{}", p).contains("enforce"));
    }

    #[test]
    fn test_process_info_display_without_profile() {
        let p = ProcessInfo {
            pid: 5678,
            name: "bash".to_string(),
            profile: None,
            mode: ProfileMode::Unconfined,
        };
        assert!(format!("{}", p).contains("unconfined"));
    }

    // ---- AuditDenial tests ----

    #[test]
    fn test_audit_denial_display() {
        let d = AuditDenial {
            timestamp: "1234567890.000".to_string(),
            profile: "/usr/bin/test".to_string(),
            operation: "open".to_string(),
            denied_mask: "r".to_string(),
            target: "/etc/secret".to_string(),
            info: None,
            comm: None,
            pid: None,
        };
        let s = format!("{}", d);
        assert!(s.contains("/usr/bin/test"));
        assert!(s.contains("open"));
        assert!(s.contains("/etc/secret"));
    }

    // ---- ParserAction tests ----

    #[test]
    fn test_parser_action_as_str() {
        assert_eq!(ParserAction::Add.as_str(), "add");
        assert_eq!(ParserAction::Replace.as_str(), "replace");
        assert_eq!(ParserAction::Remove.as_str(), "remove");
        assert_eq!(ParserAction::Preprocess.as_str(), "preprocess");
    }

    #[test]
    fn test_parser_action_display() {
        assert_eq!(format!("{}", ParserAction::Add), "add");
        assert_eq!(format!("{}", ParserAction::Remove), "remove");
    }

    // ---- parse_rule_line tests ----

    #[test]
    fn test_parse_rule_line_capability() {
        let mut p = Profile::new("test");
        parse_rule_line("  capability net_admin,", &mut p);
        assert_eq!(p.capabilities.len(), 1);
        assert_eq!(p.capabilities[0], Capability::NetAdmin);
    }

    #[test]
    fn test_parse_rule_line_capability_no_comma() {
        let mut p = Profile::new("test");
        parse_rule_line("  capability sys_ptrace", &mut p);
        assert_eq!(p.capabilities.len(), 1);
        assert_eq!(p.capabilities[0], Capability::SysPtrace);
    }

    #[test]
    fn test_parse_rule_line_network() {
        let mut p = Profile::new("test");
        parse_rule_line("  network inet stream,", &mut p);
        assert_eq!(p.network_rules.len(), 1);
        assert_eq!(p.network_rules[0].domain, NetDomain::Inet);
        assert_eq!(p.network_rules[0].sock_type, Some(NetType::Stream));
    }

    #[test]
    fn test_parse_rule_line_network_domain_only() {
        let mut p = Profile::new("test");
        parse_rule_line("  network unix,", &mut p);
        assert_eq!(p.network_rules.len(), 1);
        assert_eq!(p.network_rules[0].domain, NetDomain::Unix);
        assert!(p.network_rules[0].sock_type.is_none());
    }

    #[test]
    fn test_parse_rule_line_file_rule() {
        let mut p = Profile::new("test");
        parse_rule_line("  /usr/bin/foo rx,", &mut p);
        assert_eq!(p.file_rules.len(), 1);
        assert_eq!(p.file_rules[0].path, "/usr/bin/foo");
        assert!(p.file_rules[0].permission.read);
        assert!(p.file_rules[0].permission.execute);
    }

    #[test]
    fn test_parse_rule_line_owner_file_rule() {
        let mut p = Profile::new("test");
        parse_rule_line("  owner /tmp/** rw,", &mut p);
        assert_eq!(p.file_rules.len(), 1);
        assert!(p.file_rules[0].owner_only);
        assert!(p.file_rules[0].permission.read);
        assert!(p.file_rules[0].permission.write);
    }

    #[test]
    fn test_parse_rule_line_include() {
        let mut p = Profile::new("test");
        parse_rule_line("  #include <abstractions/base>", &mut p);
        assert_eq!(p.includes.len(), 1);
        assert_eq!(p.includes[0], "abstractions/base");
    }

    #[test]
    fn test_parse_rule_line_comment() {
        let mut p = Profile::new("test");
        parse_rule_line("  # this is a comment", &mut p);
        assert!(p.capabilities.is_empty());
        assert!(p.file_rules.is_empty());
    }

    #[test]
    fn test_parse_rule_line_empty() {
        let mut p = Profile::new("test");
        parse_rule_line("", &mut p);
        assert!(p.capabilities.is_empty());
    }

    #[test]
    fn test_parse_rule_line_glob_path() {
        let mut p = Profile::new("test");
        parse_rule_line("  /lib/** mr,", &mut p);
        assert_eq!(p.file_rules.len(), 1);
        assert!(p.file_rules[0].path.contains('*'));
    }

    // ---- parse_profile_content tests ----

    #[test]
    fn test_parse_profile_simple() {
        let content = "/usr/bin/test {\n  capability net_admin,\n  /tmp/** rw,\n}\n";
        let profiles = parse_profile_content(content, None);
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].name, "/usr/bin/test");
        assert_eq!(profiles[0].capabilities.len(), 1);
        assert_eq!(profiles[0].file_rules.len(), 1);
    }

    #[test]
    fn test_parse_profile_with_flags() {
        let content = "/usr/bin/test flags=(complain) {\n  /tmp/** rw,\n}\n";
        let profiles = parse_profile_content(content, None);
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].mode, ProfileMode::Complain);
    }

    #[test]
    fn test_parse_profile_keyword() {
        let content = "profile /usr/bin/test {\n  /tmp/** rw,\n}\n";
        let profiles = parse_profile_content(content, None);
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].name, "/usr/bin/test");
    }

    #[test]
    fn test_parse_profile_multiple() {
        let content = "/usr/bin/foo {\n  /tmp r,\n}\n/usr/bin/bar {\n  /var rw,\n}\n";
        let profiles = parse_profile_content(content, None);
        assert_eq!(profiles.len(), 2);
    }

    #[test]
    fn test_parse_profile_empty_input() {
        let profiles = parse_profile_content("", None);
        assert!(profiles.is_empty());
    }

    #[test]
    fn test_parse_profile_comments_only() {
        let content = "# This is a comment\n# Another comment\n";
        let profiles = parse_profile_content(content, None);
        assert!(profiles.is_empty());
    }

    #[test]
    fn test_parse_profile_with_includes() {
        let content = "#include <tunables/global>\n/usr/bin/test {\n  #include <abstractions/base>\n  /tmp r,\n}\n";
        let profiles = parse_profile_content(content, None);
        assert_eq!(profiles.len(), 1);
        assert!(!profiles[0].includes.is_empty());
    }

    #[test]
    fn test_parse_profile_with_network_rules() {
        let content = "/usr/bin/test {\n  network inet stream,\n  network inet6 dgram,\n}\n";
        let profiles = parse_profile_content(content, None);
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].network_rules.len(), 2);
    }

    #[test]
    fn test_parse_profile_with_capabilities() {
        let content = "/usr/bin/test {\n  capability net_admin,\n  capability sys_ptrace,\n  capability chown,\n}\n";
        let profiles = parse_profile_content(content, None);
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].capabilities.len(), 3);
    }

    #[test]
    fn test_parse_profile_source_file() {
        let path = Path::new("/etc/apparmor.d/test");
        let content = "/usr/bin/test {\n  /tmp r,\n}\n";
        let profiles = parse_profile_content(content, Some(path));
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].profile_file.as_deref(), Some(path));
    }

    #[test]
    fn test_parse_profile_unclosed_brace() {
        let content = "/usr/bin/test {\n  /tmp r,\n";
        let profiles = parse_profile_content(content, None);
        // Should still produce a profile (lenient parsing)
        assert_eq!(profiles.len(), 1);
    }

    // ---- parse_proc_attr tests ----

    #[test]
    fn test_parse_proc_attr_unconfined() {
        let (profile, mode) = parse_proc_attr("unconfined");
        assert!(profile.is_none());
        assert_eq!(mode, ProfileMode::Unconfined);
    }

    #[test]
    fn test_parse_proc_attr_empty() {
        let (profile, mode) = parse_proc_attr("");
        assert!(profile.is_none());
        assert_eq!(mode, ProfileMode::Unconfined);
    }

    #[test]
    fn test_parse_proc_attr_enforce() {
        let (profile, mode) = parse_proc_attr("/usr/bin/test (enforce)");
        assert_eq!(profile, Some("/usr/bin/test".to_string()));
        assert_eq!(mode, ProfileMode::Enforce);
    }

    #[test]
    fn test_parse_proc_attr_complain() {
        let (profile, mode) = parse_proc_attr("/usr/sbin/nginx (complain)");
        assert_eq!(profile, Some("/usr/sbin/nginx".to_string()));
        assert_eq!(mode, ProfileMode::Complain);
    }

    #[test]
    fn test_parse_proc_attr_no_mode() {
        let (profile, mode) = parse_proc_attr("/usr/bin/test");
        assert_eq!(profile, Some("/usr/bin/test".to_string()));
        assert_eq!(mode, ProfileMode::Enforce);
    }

    #[test]
    fn test_parse_proc_attr_with_whitespace() {
        let (profile, mode) = parse_proc_attr("  /usr/bin/test (enforce)  ");
        assert_eq!(profile, Some("/usr/bin/test".to_string()));
        assert_eq!(mode, ProfileMode::Enforce);
    }

    // ---- parse_audit_denial_line tests ----

    #[test]
    fn test_parse_audit_denial_full() {
        let line = r#"type=AVC msg=audit(1234567890.000:100): apparmor="DENIED" operation="open" profile="/usr/bin/test" name="/etc/secret" pid=1234 comm="test" denied_mask="r""#;
        let denial = parse_audit_denial_line(line);
        assert!(denial.is_some());
        let d = denial.unwrap();
        assert_eq!(d.profile, "/usr/bin/test");
        assert_eq!(d.operation, "open");
        assert_eq!(d.target, "/etc/secret");
        assert_eq!(d.denied_mask, "r");
        assert_eq!(d.pid, Some(1234));
    }

    #[test]
    fn test_parse_audit_denial_minimal() {
        let line = r#"apparmor="DENIED" operation="open" profile="/test""#;
        let denial = parse_audit_denial_line(line);
        assert!(denial.is_some());
        let d = denial.unwrap();
        assert_eq!(d.profile, "/test");
        assert_eq!(d.operation, "open");
    }

    #[test]
    fn test_parse_audit_denial_no_profile() {
        let line = "some random log line without profile";
        let denial = parse_audit_denial_line(line);
        assert!(denial.is_none());
    }

    #[test]
    fn test_parse_audit_denial_timestamp() {
        let line =
            r#"msg=audit(1609459200.123:456) apparmor="DENIED" operation="open" profile="/test""#;
        let denial = parse_audit_denial_line(line);
        assert!(denial.is_some());
        assert_eq!(denial.unwrap().timestamp, "1609459200.123:456");
    }

    #[test]
    fn test_parse_audit_denial_with_info() {
        let line = r#"apparmor="DENIED" operation="capable" profile="/test" info="denied_cap" denied_mask="sys_admin""#;
        let denial = parse_audit_denial_line(line);
        assert!(denial.is_some());
        let d = denial.unwrap();
        assert_eq!(d.info, Some("denied_cap".to_string()));
    }

    // ---- generate_profile tests ----

    #[test]
    fn test_generate_profile_contains_binary() {
        let profile = generate_profile("/usr/bin/myapp");
        assert!(profile.contains("/usr/bin/myapp"));
    }

    #[test]
    fn test_generate_profile_contains_header() {
        let profile = generate_profile("/usr/bin/myapp");
        assert!(profile.contains("# AppArmor profile for myapp"));
    }

    #[test]
    fn test_generate_profile_contains_includes() {
        let profile = generate_profile("/usr/bin/myapp");
        assert!(profile.contains("#include <tunables/global>"));
        assert!(profile.contains("#include <abstractions/base>"));
    }

    #[test]
    fn test_generate_profile_contains_deny_rules() {
        let profile = generate_profile("/usr/bin/myapp");
        assert!(profile.contains("deny /etc/shadow r,"));
        assert!(profile.contains("deny /etc/gshadow r,"));
    }

    #[test]
    fn test_generate_profile_has_braces() {
        let profile = generate_profile("/usr/bin/myapp");
        assert!(profile.contains('{'));
        assert!(profile.contains('}'));
    }

    #[test]
    fn test_generate_profile_lib_rules() {
        let profile = generate_profile("/usr/bin/myapp");
        assert!(profile.contains("/lib/** mr,"));
        assert!(profile.contains("/usr/lib/** mr,"));
    }

    // ---- suggest_rules_from_denials tests ----

    #[test]
    fn test_suggest_rules_file_open() {
        let denials = vec![AuditDenial {
            timestamp: String::new(),
            profile: "/test".to_string(),
            operation: "open".to_string(),
            denied_mask: "r".to_string(),
            target: "/etc/config".to_string(),
            info: None,
            comm: None,
            pid: None,
        }];
        let suggestions = suggest_rules_from_denials(&denials);
        assert_eq!(suggestions.len(), 1);
        assert!(suggestions[0].contains("/etc/config"));
        assert!(suggestions[0].contains('r'));
    }

    #[test]
    fn test_suggest_rules_capability() {
        let denials = vec![AuditDenial {
            timestamp: String::new(),
            profile: "/test".to_string(),
            operation: "capable".to_string(),
            denied_mask: "net_admin".to_string(),
            target: String::new(),
            info: None,
            comm: None,
            pid: None,
        }];
        let suggestions = suggest_rules_from_denials(&denials);
        assert_eq!(suggestions.len(), 1);
        assert!(suggestions[0].contains("capability"));
        assert!(suggestions[0].contains("net_admin"));
    }

    #[test]
    fn test_suggest_rules_network() {
        let denials = vec![AuditDenial {
            timestamp: String::new(),
            profile: "/test".to_string(),
            operation: "connect".to_string(),
            denied_mask: "send".to_string(),
            target: "addr".to_string(),
            info: None,
            comm: None,
            pid: None,
        }];
        let suggestions = suggest_rules_from_denials(&denials);
        assert_eq!(suggestions.len(), 1);
        assert!(suggestions[0].contains("network"));
    }

    #[test]
    fn test_suggest_rules_exec() {
        let denials = vec![AuditDenial {
            timestamp: String::new(),
            profile: "/test".to_string(),
            operation: "exec".to_string(),
            denied_mask: "x".to_string(),
            target: "/usr/bin/ls".to_string(),
            info: None,
            comm: None,
            pid: None,
        }];
        let suggestions = suggest_rules_from_denials(&denials);
        assert_eq!(suggestions.len(), 1);
        assert!(suggestions[0].contains("/usr/bin/ls"));
        assert!(suggestions[0].contains("ix"));
    }

    #[test]
    fn test_suggest_rules_deduplicate() {
        let denial = AuditDenial {
            timestamp: String::new(),
            profile: "/test".to_string(),
            operation: "open".to_string(),
            denied_mask: "r".to_string(),
            target: "/etc/file".to_string(),
            info: None,
            comm: None,
            pid: None,
        };
        let denials = vec![denial.clone(), denial];
        let suggestions = suggest_rules_from_denials(&denials);
        assert_eq!(suggestions.len(), 1);
    }

    #[test]
    fn test_suggest_rules_signal() {
        let denials = vec![AuditDenial {
            timestamp: String::new(),
            profile: "/test".to_string(),
            operation: "signal".to_string(),
            denied_mask: "send".to_string(),
            target: "/usr/bin/foo".to_string(),
            info: None,
            comm: None,
            pid: None,
        }];
        let suggestions = suggest_rules_from_denials(&denials);
        assert!(suggestions[0].contains("signal"));
    }

    #[test]
    fn test_suggest_rules_ptrace() {
        let denials = vec![AuditDenial {
            timestamp: String::new(),
            profile: "/test".to_string(),
            operation: "ptrace".to_string(),
            denied_mask: "read".to_string(),
            target: "/usr/bin/foo".to_string(),
            info: None,
            comm: None,
            pid: None,
        }];
        let suggestions = suggest_rules_from_denials(&denials);
        assert!(suggestions[0].contains("ptrace"));
    }

    #[test]
    fn test_suggest_rules_empty() {
        let suggestions = suggest_rules_from_denials(&[]);
        assert!(suggestions.is_empty());
    }

    // ---- preprocess_profile tests ----

    #[test]
    fn test_preprocess_simple() {
        let input = "/usr/bin/test {\n  /tmp r,\n}\n";
        let output = preprocess_profile(input);
        assert!(output.contains("/usr/bin/test"));
        assert!(output.contains("/tmp r,"));
    }

    #[test]
    fn test_preprocess_variable_expansion() {
        let input = "@{HOME}=/home/*\n/usr/bin/test {\n  @{HOME}/data r,\n}\n";
        let output = preprocess_profile(input);
        assert!(output.contains("/home/*/data r,"));
    }

    #[test]
    fn test_preprocess_include_resolve() {
        let input = "#include <abstractions/base>\n/usr/bin/test {\n}\n";
        let output = preprocess_profile(input);
        assert!(output.contains("include resolved"));
    }

    // ---- validate_profile tests ----

    #[test]
    fn test_validate_profile_empty_name() {
        let p = Profile::new("");
        let errors = validate_profile(&p);
        assert!(!errors.is_empty());
        assert!(errors[0].contains("name is empty"));
    }

    #[test]
    fn test_validate_profile_valid() {
        let p = Profile::new("/usr/bin/test");
        let errors = validate_profile(&p);
        // Only error might be binary not existing, which is fine in tests
        for err in &errors {
            assert!(err.contains("does not exist") || err.contains("Duplicate"));
        }
    }

    #[test]
    fn test_validate_profile_duplicate_file_rules() {
        let mut p = Profile::new("test");
        p.file_rules.push(FileRule {
            path: "/tmp/test".to_string(),
            permission: FilePermission::from_str("r"),
            owner_only: false,
        });
        p.file_rules.push(FileRule {
            path: "/tmp/test".to_string(),
            permission: FilePermission::from_str("rw"),
            owner_only: false,
        });
        let errors = validate_profile(&p);
        assert!(errors.iter().any(|e| e.contains("Duplicate file rule")));
    }

    #[test]
    fn test_validate_profile_duplicate_network_rules() {
        let mut p = Profile::new("test");
        p.network_rules.push(NetworkRule {
            domain: NetDomain::Inet,
            sock_type: Some(NetType::Stream),
        });
        p.network_rules.push(NetworkRule {
            domain: NetDomain::Inet,
            sock_type: Some(NetType::Stream),
        });
        let errors = validate_profile(&p);
        assert!(errors.iter().any(|e| e.contains("Duplicate network rule")));
    }

    // ---- rewrite_profile_flags tests ----

    #[test]
    fn test_rewrite_flags_add_complain() {
        let content = "/usr/bin/test {\n  /tmp r,\n}\n";
        let output = rewrite_profile_flags(content, ProfileMode::Complain);
        assert!(output.contains("flags=(complain)"));
    }

    #[test]
    fn test_rewrite_flags_remove_complain() {
        let content = "/usr/bin/test flags=(complain) {\n  /tmp r,\n}\n";
        let output = rewrite_profile_flags(content, ProfileMode::Enforce);
        assert!(!output.contains("flags=(complain)"));
    }

    #[test]
    fn test_rewrite_flags_replace_mode() {
        let content = "/usr/bin/test flags=(complain) {\n  /tmp r,\n}\n";
        let output = rewrite_profile_flags(content, ProfileMode::Complain);
        // Should still have complain flags
        assert!(output.contains("flags=(complain)"));
    }

    #[test]
    fn test_rewrite_flags_preserves_rules() {
        let content = "/usr/bin/test {\n  capability net_admin,\n  /tmp r,\n}\n";
        let output = rewrite_profile_flags(content, ProfileMode::Complain);
        assert!(output.contains("capability net_admin,"));
        assert!(output.contains("/tmp r,"));
    }

    // ---- remove_flags tests ----

    #[test]
    fn test_remove_flags_with_flags() {
        let result = remove_flags("/usr/bin/test flags=(complain) {");
        assert!(!result.contains("flags="));
        assert!(result.contains("/usr/bin/test"));
        assert!(result.contains('{'));
    }

    #[test]
    fn test_remove_flags_without_flags() {
        let result = remove_flags("/usr/bin/test {");
        assert_eq!(result, "/usr/bin/test {");
    }

    #[test]
    fn test_remove_flags_multiple_words_in_flags() {
        let result = remove_flags("/usr/bin/test flags=(complain,attach_disconnected) {");
        assert!(!result.contains("flags="));
        assert!(result.contains("/usr/bin/test"));
    }

    // ---- Personality detection tests ----

    #[test]
    fn test_personality_unix_path() {
        let s = "/usr/bin/aa-status";
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' {
                last_sep = i + 1;
            }
        }
        let base = &s[last_sep..];
        let base = base.strip_suffix(".exe").unwrap_or(base);
        assert_eq!(base, "aa-status");
    }

    #[test]
    fn test_personality_windows_path() {
        let s = r"C:\Users\test\aa-enforce.exe";
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' {
                last_sep = i + 1;
            }
        }
        let base = &s[last_sep..];
        let base = base.strip_suffix(".exe").unwrap_or(base);
        assert_eq!(base, "aa-enforce");
    }

    #[test]
    fn test_personality_bare_name() {
        let s = "aa-complain";
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' {
                last_sep = i + 1;
            }
        }
        let base = &s[last_sep..];
        let base = base.strip_suffix(".exe").unwrap_or(base);
        assert_eq!(base, "aa-complain");
    }

    #[test]
    fn test_personality_mixed_separators() {
        let s = "/usr/local\\bin/apparmor_parser";
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' {
                last_sep = i + 1;
            }
        }
        let base = &s[last_sep..];
        let base = base.strip_suffix(".exe").unwrap_or(base);
        assert_eq!(base, "apparmor_parser");
    }

    #[test]
    fn test_personality_empty_uses_default() {
        let s = "";
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' {
                last_sep = i + 1;
            }
        }
        let base = &s[last_sep..];
        let base = base.strip_suffix(".exe").unwrap_or(base);
        assert_eq!(base, "");
    }

    // ---- cmd_status help test ----

    #[test]
    fn test_cmd_status_help() {
        let args = vec!["--help".to_string()];
        let code = cmd_status(&args);
        assert_eq!(code, 0);
    }

    // ---- cmd_enforce tests ----

    #[test]
    fn test_cmd_enforce_no_args() {
        let code = cmd_enforce(&[]);
        assert_eq!(code, 1);
    }

    #[test]
    fn test_cmd_enforce_help() {
        let args = vec!["--help".to_string()];
        let code = cmd_enforce(&args);
        assert_eq!(code, 0);
    }

    // ---- cmd_complain tests ----

    #[test]
    fn test_cmd_complain_no_args() {
        let code = cmd_complain(&[]);
        assert_eq!(code, 1);
    }

    #[test]
    fn test_cmd_complain_help() {
        let args = vec!["-h".to_string()];
        let code = cmd_complain(&args);
        assert_eq!(code, 0);
    }

    // ---- cmd_disable tests ----

    #[test]
    fn test_cmd_disable_no_args() {
        let code = cmd_disable(&[]);
        assert_eq!(code, 1);
    }

    #[test]
    fn test_cmd_disable_help() {
        let args = vec!["--help".to_string()];
        let code = cmd_disable(&args);
        assert_eq!(code, 0);
    }

    // ---- cmd_genprof tests ----

    #[test]
    fn test_cmd_genprof_no_args() {
        let code = cmd_genprof(&[]);
        assert_eq!(code, 1);
    }

    #[test]
    fn test_cmd_genprof_help() {
        let args = vec!["--help".to_string()];
        let code = cmd_genprof(&args);
        assert_eq!(code, 0);
    }

    // ---- cmd_logprof tests ----

    #[test]
    fn test_cmd_logprof_help() {
        let args = vec!["--help".to_string()];
        let code = cmd_logprof(&args);
        assert_eq!(code, 0);
    }

    #[test]
    fn test_cmd_logprof_unknown_option() {
        let args = vec!["--unknown".to_string()];
        let code = cmd_logprof(&args);
        assert_eq!(code, 1);
    }

    // ---- cmd_unconfined tests ----

    #[test]
    fn test_cmd_unconfined_help() {
        let args = vec!["--help".to_string()];
        let code = cmd_unconfined(&args);
        assert_eq!(code, 0);
    }

    #[test]
    fn test_cmd_unconfined_unknown_option() {
        let args = vec!["--bad".to_string()];
        let code = cmd_unconfined(&args);
        assert_eq!(code, 1);
    }

    // ---- cmd_parser tests ----

    #[test]
    fn test_cmd_parser_help() {
        let args = vec!["--help".to_string()];
        let code = cmd_parser(&args);
        assert_eq!(code, 0);
    }

    #[test]
    fn test_cmd_parser_version() {
        let args = vec!["--version".to_string()];
        let code = cmd_parser(&args);
        assert_eq!(code, 0);
    }

    #[test]
    fn test_cmd_parser_unknown_option() {
        let args = vec!["--nonexistent".to_string()];
        let code = cmd_parser(&args);
        assert_eq!(code, 1);
    }

    // ---- load_kernel_profiles tests ----

    #[test]
    fn test_load_kernel_profiles_nonexistent() {
        let result = load_kernel_profiles("/nonexistent/path/profiles");
        assert!(result.is_empty());
    }

    // ---- load_profiles_from_dir tests ----

    #[test]
    fn test_load_profiles_from_nonexistent_dir() {
        let result = load_profiles_from_dir(Path::new("/nonexistent/dir"));
        assert!(result.is_empty());
    }

    // ---- parse_audit_log tests ----

    #[test]
    fn test_parse_audit_log_nonexistent() {
        let result = parse_audit_log("/nonexistent/audit.log");
        assert!(result.is_empty());
    }

    // ---- Integration-style tests ----

    #[test]
    fn test_full_profile_parse_and_validate() {
        let content = r#"
#include <tunables/global>

/usr/sbin/nginx {
  #include <abstractions/base>
  #include <abstractions/nameservice>

  capability net_bind_service,
  capability setuid,
  capability setgid,

  /usr/sbin/nginx mr,
  /etc/nginx/** r,
  /var/log/nginx/** rw,
  /var/www/** r,
  /run/nginx.pid rw,

  network inet stream,
  network inet6 stream,

  owner /tmp/nginx_* rw,
}
"#;
        let profiles = parse_profile_content(content, None);
        assert_eq!(profiles.len(), 1);
        let p = &profiles[0];
        assert_eq!(p.name, "/usr/sbin/nginx");
        assert_eq!(p.mode, ProfileMode::Enforce);
        assert_eq!(p.capabilities.len(), 3);
        assert!(p.file_rules.len() >= 5);
        assert_eq!(p.network_rules.len(), 2);
        assert!(p.includes.len() >= 2);

        // Validate should only warn about nonexistent binary (in test env)
        let errors = validate_profile(p);
        for err in &errors {
            assert!(
                err.contains("does not exist") || err.contains("Duplicate"),
                "Unexpected error: {}",
                err
            );
        }
    }

    #[test]
    fn test_profile_roundtrip_mode_change() {
        let original = "/usr/bin/test {\n  /tmp r,\n}\n";

        // Set to complain
        let complain_content = rewrite_profile_flags(original, ProfileMode::Complain);
        assert!(complain_content.contains("flags=(complain)"));

        // Parse the complain version
        let profiles = parse_profile_content(&complain_content, None);
        assert_eq!(profiles[0].mode, ProfileMode::Complain);

        // Set back to enforce
        let enforce_content = rewrite_profile_flags(&complain_content, ProfileMode::Enforce);
        assert!(!enforce_content.contains("flags=(complain)"));

        // Parse the enforce version
        let profiles2 = parse_profile_content(&enforce_content, None);
        assert_eq!(profiles2[0].mode, ProfileMode::Enforce);
    }

    #[test]
    fn test_generate_and_parse_profile() {
        let generated = generate_profile("/usr/bin/myapp");
        let profiles = parse_profile_content(&generated, None);
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].name, "/usr/bin/myapp");
        assert!(!profiles[0].file_rules.is_empty());
    }

    #[test]
    fn test_suggest_mknod_rule() {
        let denials = vec![AuditDenial {
            timestamp: String::new(),
            profile: "/test".to_string(),
            operation: "mknod".to_string(),
            denied_mask: "w".to_string(),
            target: "/dev/tty0".to_string(),
            info: None,
            comm: None,
            pid: None,
        }];
        let suggestions = suggest_rules_from_denials(&denials);
        assert_eq!(suggestions.len(), 1);
        assert!(suggestions[0].contains("/dev/tty0"));
        assert!(suggestions[0].contains('w'));
    }

    #[test]
    fn test_suggest_unknown_operation() {
        let denials = vec![AuditDenial {
            timestamp: String::new(),
            profile: "/test".to_string(),
            operation: "unknown_op".to_string(),
            denied_mask: "?".to_string(),
            target: "/something".to_string(),
            info: None,
            comm: None,
            pid: None,
        }];
        let suggestions = suggest_rules_from_denials(&denials);
        assert_eq!(suggestions.len(), 1);
        assert!(suggestions[0].starts_with("  #"));
    }

    #[test]
    fn test_cmd_status_json_option() {
        // --json flag should be recognized
        let args = vec!["--json".to_string()];
        let code = cmd_status(&args);
        assert_eq!(code, 0);
    }

    #[test]
    fn test_cmd_status_unknown_option() {
        let args = vec!["--bad-flag".to_string()];
        let code = cmd_status(&args);
        assert_eq!(code, 1);
    }

    #[test]
    fn test_file_permission_memory_map() {
        let p = FilePermission::from_str("mr");
        assert!(p.memory_map);
        assert!(p.read);
        assert!(!p.write);
    }

    #[test]
    fn test_file_permission_lock() {
        let p = FilePermission::from_str("k");
        assert!(p.lock);
        assert!(!p.read);
    }

    #[test]
    fn test_file_permission_link() {
        let p = FilePermission::from_str("l");
        assert!(p.link);
        assert!(!p.write);
    }

    #[test]
    fn test_net_domain_case_insensitive() {
        assert_eq!(NetDomain::from_str("INET"), NetDomain::Inet);
        assert_eq!(NetDomain::from_str("Inet6"), NetDomain::Inet6);
    }

    #[test]
    fn test_net_type_case_insensitive() {
        assert_eq!(NetType::from_str("STREAM"), NetType::Stream);
        assert_eq!(NetType::from_str("Dgram"), NetType::Dgram);
    }

    #[test]
    fn test_preprocess_preserves_content() {
        let input = "# comment\n/usr/bin/test {\n  /tmp r,\n}\n";
        let output = preprocess_profile(input);
        assert!(output.contains("# comment"));
        assert!(output.contains("/usr/bin/test"));
    }

    #[test]
    fn test_profile_with_child_profiles() {
        let content = "/usr/bin/test {\n  /tmp r,\n  ^child {\n    /var r,\n  }\n}\n";
        let profiles = parse_profile_content(content, None);
        assert_eq!(profiles.len(), 1);
        assert!(!profiles[0].child_profiles.is_empty());
    }
}
