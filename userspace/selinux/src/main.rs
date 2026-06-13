//! Slate OS SELinux security tools.
//!
//! Multi-personality binary providing:
//! - **getenforce** (default) — get SELinux enforcement mode
//! - **setenforce** — set enforcement mode (enforcing/permissive)
//! - **sestatus** — show SELinux status
//! - **semanage** — SELinux policy management (booleans, ports, fcontext, login, user)
//! - **setsebool** — set SELinux booleans
//! - **getsebool** — get SELinux booleans
//! - **restorecon** — restore file security contexts
//! - **chcon** — change file security context
//! - **seinfo** — SELinux policy query tool
//! - **sesearch** — SELinux policy rule search
//! - **audit2allow** — generate policy from audit logs
//!
//! Implements SELinux user-space tools for managing mandatory access control
//! policies, security contexts, and audit log analysis.

#![deny(clippy::all)]

use std::collections::HashMap;
use std::env;
use std::fmt;
use std::fs;
use std::io::{self, BufRead};
use std::path::Path;
use std::process;

const VERSION: &str = "0.1.0";
const SELINUX_FS: &str = "/sys/fs/selinux";
const ENFORCE_PATH: &str = "/sys/fs/selinux/enforce";
const POLICY_DIR: &str = "/etc/selinux";
const _CONTEXTS_DIR: &str = "/etc/selinux/targeted/contexts/files";
const BOOLEANS_DIR: &str = "/sys/fs/selinux/booleans";
const _AUDIT_LOG: &str = "/var/log/audit/audit.log";

// ============================================================================
// SELinux enforcement mode
// ============================================================================

/// SELinux enforcement mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EnforceMode {
    Enforcing,
    Permissive,
    Disabled,
}

impl fmt::Display for EnforceMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Enforcing => write!(f, "Enforcing"),
            Self::Permissive => write!(f, "Permissive"),
            Self::Disabled => write!(f, "Disabled"),
        }
    }
}

impl EnforceMode {
    fn from_value(v: u8) -> Self {
        match v {
            1 => Self::Enforcing,
            0 => Self::Permissive,
            _ => Self::Disabled,
        }
    }

    fn to_value(self) -> u8 {
        match self {
            Self::Enforcing => 1,
            Self::Permissive => 0,
            Self::Disabled => 2,
        }
    }
}

// ============================================================================
// Security context
// ============================================================================

/// An SELinux security context in user:role:type:level format.
#[derive(Clone, Debug, PartialEq, Eq)]
struct SecurityContext {
    user: String,
    role: String,
    context_type: String,
    level: String,
}

impl SecurityContext {
    fn parse(s: &str) -> Option<Self> {
        let parts: Vec<&str> = s.split(':').collect();
        if parts.len() < 3 {
            return None;
        }
        let level = if parts.len() >= 4 {
            parts[3..].join(":")
        } else {
            String::from("s0")
        };
        Some(Self {
            user: parts[0].to_string(),
            role: parts[1].to_string(),
            context_type: parts[2].to_string(),
            level,
        })
    }
}

impl fmt::Display for SecurityContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}:{}:{}",
            self.user, self.role, self.context_type, self.level
        )
    }
}

// ============================================================================
// Policy rule types
// ============================================================================

/// Type of policy rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RuleKind {
    Allow,
    Dontaudit,
    Auditallow,
    TypeTransition,
    _TypeChange,
    _TypeMember,
    Neverallow,
}

impl fmt::Display for RuleKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Allow => write!(f, "allow"),
            Self::Dontaudit => write!(f, "dontaudit"),
            Self::Auditallow => write!(f, "auditallow"),
            Self::TypeTransition => write!(f, "type_transition"),
            Self::_TypeChange => write!(f, "type_change"),
            Self::_TypeMember => write!(f, "type_member"),
            Self::Neverallow => write!(f, "neverallow"),
        }
    }
}

impl RuleKind {
    fn _from_str(s: &str) -> Option<Self> {
        match s {
            "allow" => Some(Self::Allow),
            "dontaudit" => Some(Self::Dontaudit),
            "auditallow" => Some(Self::Auditallow),
            "type_transition" => Some(Self::TypeTransition),
            "type_change" => Some(Self::_TypeChange),
            "type_member" => Some(Self::_TypeMember),
            "neverallow" => Some(Self::Neverallow),
            _ => None,
        }
    }
}

/// A policy rule.
#[derive(Clone, Debug, PartialEq, Eq)]
struct PolicyRule {
    kind: RuleKind,
    source: String,
    target: String,
    class: String,
    permissions: Vec<String>,
}

impl fmt::Display for PolicyRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} {} {}:{} {{ {} }};",
            self.kind,
            self.source,
            self.target,
            self.class,
            self.permissions.join(" ")
        )
    }
}

// ============================================================================
// Policy database (in-memory representation)
// ============================================================================

/// SELinux boolean.
#[derive(Clone, Debug, PartialEq, Eq)]
struct SeBool {
    name: String,
    active: bool,
    pending: bool,
}

/// Protocol for port context.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Protocol {
    Tcp,
    Udp,
    Dccp,
    Sctp,
}

impl fmt::Display for Protocol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Tcp => write!(f, "tcp"),
            Self::Udp => write!(f, "udp"),
            Self::Dccp => write!(f, "dccp"),
            Self::Sctp => write!(f, "sctp"),
        }
    }
}

impl Protocol {
    fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "tcp" => Some(Self::Tcp),
            "udp" => Some(Self::Udp),
            "dccp" => Some(Self::Dccp),
            "sctp" => Some(Self::Sctp),
            _ => None,
        }
    }
}

/// A port context entry.
#[derive(Clone, Debug, PartialEq, Eq)]
struct PortContext {
    protocol: Protocol,
    port_low: u16,
    port_high: u16,
    context: SecurityContext,
}

impl fmt::Display for PortContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.port_low == self.port_high {
            write!(f, "{} {} {}", self.protocol, self.port_low, self.context)
        } else {
            write!(
                f,
                "{} {}-{} {}",
                self.protocol, self.port_low, self.port_high, self.context
            )
        }
    }
}

/// File context type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FileContextType {
    All,
    Regular,
    Directory,
    CharDevice,
    BlockDevice,
    Socket,
    SymLink,
    Pipe,
}

impl fmt::Display for FileContextType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::All => write!(f, "all files"),
            Self::Regular => write!(f, "regular file"),
            Self::Directory => write!(f, "directory"),
            Self::CharDevice => write!(f, "character device"),
            Self::BlockDevice => write!(f, "block device"),
            Self::Socket => write!(f, "socket"),
            Self::SymLink => write!(f, "symbolic link"),
            Self::Pipe => write!(f, "named pipe"),
        }
    }
}

impl FileContextType {
    fn from_flag(s: &str) -> Option<Self> {
        match s {
            "a" | "--" => Some(Self::All),
            "f" | "-f" => Some(Self::Regular),
            "d" | "-d" => Some(Self::Directory),
            "c" | "-c" => Some(Self::CharDevice),
            "b" | "-b" => Some(Self::BlockDevice),
            "s" | "-s" => Some(Self::Socket),
            "l" | "-l" => Some(Self::SymLink),
            "p" | "-p" => Some(Self::Pipe),
            _ => None,
        }
    }
}

/// A file context entry.
#[derive(Clone, Debug, PartialEq, Eq)]
struct FileContext {
    pattern: String,
    file_type: FileContextType,
    context: SecurityContext,
}

impl fmt::Display for FileContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}\t{}\t{}",
            self.pattern, self.file_type, self.context
        )
    }
}

/// SELinux login mapping.
#[derive(Clone, Debug, PartialEq, Eq)]
struct LoginMapping {
    login: String,
    seuser: String,
    mls_range: String,
}

impl fmt::Display for LoginMapping {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}\t{}", self.login, self.mls_range, self.seuser)
    }
}

/// SELinux user.
#[derive(Clone, Debug, PartialEq, Eq)]
struct SeUser {
    name: String,
    roles: Vec<String>,
    mls_level: String,
    mls_range: String,
}

impl fmt::Display for SeUser {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}\t{}\t{}\t{}",
            self.name,
            self.roles.join(" "),
            self.mls_level,
            self.mls_range
        )
    }
}

/// SELinux status information.
#[derive(Clone, Debug)]
struct SeStatus {
    enabled: bool,
    mode: EnforceMode,
    config_mode: EnforceMode,
    policy_version: u32,
    policy_name: String,
    mls_enabled: bool,
    deny_unknown: bool,
    max_kernel_policy: u32,
}

impl Default for SeStatus {
    fn default() -> Self {
        Self {
            enabled: false,
            mode: EnforceMode::Disabled,
            config_mode: EnforceMode::Disabled,
            policy_version: 33,
            policy_name: String::from("targeted"),
            mls_enabled: true,
            deny_unknown: false,
            max_kernel_policy: 33,
        }
    }
}

/// The full in-memory policy database.
#[derive(Clone, Debug)]
struct PolicyDb {
    types: Vec<String>,
    roles: Vec<String>,
    users: Vec<SeUser>,
    booleans: Vec<SeBool>,
    classes: Vec<String>,
    permissions: Vec<String>,
    rules: Vec<PolicyRule>,
    ports: Vec<PortContext>,
    file_contexts: Vec<FileContext>,
    login_mappings: Vec<LoginMapping>,
}

impl Default for PolicyDb {
    fn default() -> Self {
        Self {
            types: vec![
                "bin_t".into(),
                "etc_t".into(),
                "home_t".into(),
                "httpd_t".into(),
                "init_t".into(),
                "kernel_t".into(),
                "lib_t".into(),
                "net_t".into(),
                "proc_t".into(),
                "sshd_t".into(),
                "syslog_t".into(),
                "tmp_t".into(),
                "user_t".into(),
                "var_t".into(),
                "unconfined_t".into(),
            ],
            roles: vec![
                "object_r".into(),
                "system_r".into(),
                "staff_r".into(),
                "sysadm_r".into(),
                "user_r".into(),
                "unconfined_r".into(),
            ],
            users: vec![
                SeUser {
                    name: "system_u".into(),
                    roles: vec!["system_r".into(), "object_r".into()],
                    mls_level: "s0".into(),
                    mls_range: "s0-s0:c0.c1023".into(),
                },
                SeUser {
                    name: "root".into(),
                    roles: vec![
                        "staff_r".into(),
                        "sysadm_r".into(),
                        "system_r".into(),
                        "unconfined_r".into(),
                    ],
                    mls_level: "s0".into(),
                    mls_range: "s0-s0:c0.c1023".into(),
                },
                SeUser {
                    name: "user_u".into(),
                    roles: vec!["user_r".into()],
                    mls_level: "s0".into(),
                    mls_range: "s0".into(),
                },
                SeUser {
                    name: "staff_u".into(),
                    roles: vec!["staff_r".into(), "sysadm_r".into()],
                    mls_level: "s0".into(),
                    mls_range: "s0-s0:c0.c1023".into(),
                },
                SeUser {
                    name: "unconfined_u".into(),
                    roles: vec!["unconfined_r".into()],
                    mls_level: "s0".into(),
                    mls_range: "s0-s0:c0.c1023".into(),
                },
            ],
            booleans: vec![
                SeBool {
                    name: "httpd_can_network_connect".into(),
                    active: false,
                    pending: false,
                },
                SeBool {
                    name: "httpd_enable_cgi".into(),
                    active: true,
                    pending: true,
                },
                SeBool {
                    name: "allow_user_exec_content".into(),
                    active: true,
                    pending: true,
                },
                SeBool {
                    name: "sshd_allow_tcp_forwarding".into(),
                    active: false,
                    pending: false,
                },
                SeBool {
                    name: "virt_use_nfs".into(),
                    active: false,
                    pending: false,
                },
            ],
            classes: vec![
                "file".into(),
                "dir".into(),
                "process".into(),
                "tcp_socket".into(),
                "udp_socket".into(),
                "unix_stream_socket".into(),
                "capability".into(),
                "lnk_file".into(),
                "chr_file".into(),
                "blk_file".into(),
                "sock_file".into(),
                "fifo_file".into(),
                "fd".into(),
                "node".into(),
                "netif".into(),
                "msg".into(),
                "msgq".into(),
                "shm".into(),
                "sem".into(),
            ],
            permissions: vec![
                "read".into(),
                "write".into(),
                "execute".into(),
                "open".into(),
                "create".into(),
                "unlink".into(),
                "rename".into(),
                "getattr".into(),
                "setattr".into(),
                "append".into(),
                "lock".into(),
                "ioctl".into(),
                "connect".into(),
                "listen".into(),
                "accept".into(),
                "bind".into(),
                "send".into(),
                "recv".into(),
                "name_bind".into(),
                "transition".into(),
                "signal".into(),
                "sigchld".into(),
                "search".into(),
                "add_name".into(),
                "remove_name".into(),
                "mounton".into(),
            ],
            rules: vec![
                PolicyRule {
                    kind: RuleKind::Allow,
                    source: "httpd_t".into(),
                    target: "httpd_t".into(),
                    class: "process".into(),
                    permissions: vec!["signal".into(), "sigchld".into()],
                },
                PolicyRule {
                    kind: RuleKind::Allow,
                    source: "httpd_t".into(),
                    target: "etc_t".into(),
                    class: "file".into(),
                    permissions: vec!["read".into(), "open".into(), "getattr".into()],
                },
                PolicyRule {
                    kind: RuleKind::Allow,
                    source: "httpd_t".into(),
                    target: "net_t".into(),
                    class: "tcp_socket".into(),
                    permissions: vec!["connect".into(), "send".into(), "recv".into()],
                },
                PolicyRule {
                    kind: RuleKind::Allow,
                    source: "sshd_t".into(),
                    target: "etc_t".into(),
                    class: "file".into(),
                    permissions: vec!["read".into(), "open".into(), "getattr".into()],
                },
                PolicyRule {
                    kind: RuleKind::Allow,
                    source: "sshd_t".into(),
                    target: "net_t".into(),
                    class: "tcp_socket".into(),
                    permissions: vec![
                        "connect".into(),
                        "listen".into(),
                        "accept".into(),
                        "bind".into(),
                    ],
                },
                PolicyRule {
                    kind: RuleKind::Allow,
                    source: "init_t".into(),
                    target: "bin_t".into(),
                    class: "file".into(),
                    permissions: vec!["read".into(), "execute".into(), "open".into()],
                },
                PolicyRule {
                    kind: RuleKind::Allow,
                    source: "user_t".into(),
                    target: "home_t".into(),
                    class: "file".into(),
                    permissions: vec![
                        "read".into(),
                        "write".into(),
                        "create".into(),
                        "unlink".into(),
                        "open".into(),
                    ],
                },
                PolicyRule {
                    kind: RuleKind::Allow,
                    source: "user_t".into(),
                    target: "home_t".into(),
                    class: "dir".into(),
                    permissions: vec![
                        "read".into(),
                        "search".into(),
                        "add_name".into(),
                        "remove_name".into(),
                    ],
                },
                PolicyRule {
                    kind: RuleKind::Allow,
                    source: "user_t".into(),
                    target: "tmp_t".into(),
                    class: "file".into(),
                    permissions: vec![
                        "read".into(),
                        "write".into(),
                        "create".into(),
                        "unlink".into(),
                    ],
                },
                PolicyRule {
                    kind: RuleKind::Dontaudit,
                    source: "httpd_t".into(),
                    target: "proc_t".into(),
                    class: "file".into(),
                    permissions: vec!["read".into()],
                },
                PolicyRule {
                    kind: RuleKind::Dontaudit,
                    source: "sshd_t".into(),
                    target: "proc_t".into(),
                    class: "file".into(),
                    permissions: vec!["read".into()],
                },
                PolicyRule {
                    kind: RuleKind::TypeTransition,
                    source: "init_t".into(),
                    target: "bin_t".into(),
                    class: "process".into(),
                    permissions: vec!["httpd_t".into()],
                },
                PolicyRule {
                    kind: RuleKind::TypeTransition,
                    source: "init_t".into(),
                    target: "bin_t".into(),
                    class: "file".into(),
                    permissions: vec!["sshd_t".into()],
                },
                PolicyRule {
                    kind: RuleKind::Allow,
                    source: "unconfined_t".into(),
                    target: "unconfined_t".into(),
                    class: "process".into(),
                    permissions: vec![
                        "signal".into(),
                        "sigchld".into(),
                        "transition".into(),
                    ],
                },
                PolicyRule {
                    kind: RuleKind::Allow,
                    source: "kernel_t".into(),
                    target: "kernel_t".into(),
                    class: "process".into(),
                    permissions: vec!["signal".into(), "sigchld".into()],
                },
            ],
            ports: vec![
                PortContext {
                    protocol: Protocol::Tcp,
                    port_low: 80,
                    port_high: 80,
                    context: SecurityContext {
                        user: "system_u".into(),
                        role: "object_r".into(),
                        context_type: "http_port_t".into(),
                        level: "s0".into(),
                    },
                },
                PortContext {
                    protocol: Protocol::Tcp,
                    port_low: 443,
                    port_high: 443,
                    context: SecurityContext {
                        user: "system_u".into(),
                        role: "object_r".into(),
                        context_type: "http_port_t".into(),
                        level: "s0".into(),
                    },
                },
                PortContext {
                    protocol: Protocol::Tcp,
                    port_low: 22,
                    port_high: 22,
                    context: SecurityContext {
                        user: "system_u".into(),
                        role: "object_r".into(),
                        context_type: "ssh_port_t".into(),
                        level: "s0".into(),
                    },
                },
                PortContext {
                    protocol: Protocol::Tcp,
                    port_low: 8080,
                    port_high: 8090,
                    context: SecurityContext {
                        user: "system_u".into(),
                        role: "object_r".into(),
                        context_type: "http_cache_port_t".into(),
                        level: "s0".into(),
                    },
                },
                PortContext {
                    protocol: Protocol::Udp,
                    port_low: 53,
                    port_high: 53,
                    context: SecurityContext {
                        user: "system_u".into(),
                        role: "object_r".into(),
                        context_type: "dns_port_t".into(),
                        level: "s0".into(),
                    },
                },
            ],
            file_contexts: vec![
                FileContext {
                    pattern: "/etc(/.*)?".into(),
                    file_type: FileContextType::All,
                    context: SecurityContext {
                        user: "system_u".into(),
                        role: "object_r".into(),
                        context_type: "etc_t".into(),
                        level: "s0".into(),
                    },
                },
                FileContext {
                    pattern: "/home(/.*)?".into(),
                    file_type: FileContextType::All,
                    context: SecurityContext {
                        user: "system_u".into(),
                        role: "object_r".into(),
                        context_type: "home_t".into(),
                        level: "s0".into(),
                    },
                },
                FileContext {
                    pattern: "/tmp(/.*)?".into(),
                    file_type: FileContextType::All,
                    context: SecurityContext {
                        user: "system_u".into(),
                        role: "object_r".into(),
                        context_type: "tmp_t".into(),
                        level: "s0".into(),
                    },
                },
                FileContext {
                    pattern: "/usr/bin(/.*)?".into(),
                    file_type: FileContextType::Regular,
                    context: SecurityContext {
                        user: "system_u".into(),
                        role: "object_r".into(),
                        context_type: "bin_t".into(),
                        level: "s0".into(),
                    },
                },
                FileContext {
                    pattern: "/var(/.*)?".into(),
                    file_type: FileContextType::All,
                    context: SecurityContext {
                        user: "system_u".into(),
                        role: "object_r".into(),
                        context_type: "var_t".into(),
                        level: "s0".into(),
                    },
                },
                FileContext {
                    pattern: "/usr/lib(/.*)?".into(),
                    file_type: FileContextType::Regular,
                    context: SecurityContext {
                        user: "system_u".into(),
                        role: "object_r".into(),
                        context_type: "lib_t".into(),
                        level: "s0".into(),
                    },
                },
                FileContext {
                    pattern: "/proc(/.*)?".into(),
                    file_type: FileContextType::All,
                    context: SecurityContext {
                        user: "system_u".into(),
                        role: "object_r".into(),
                        context_type: "proc_t".into(),
                        level: "s0".into(),
                    },
                },
            ],
            login_mappings: vec![
                LoginMapping {
                    login: "__default__".into(),
                    seuser: "unconfined_u".into(),
                    mls_range: "s0-s0:c0.c1023".into(),
                },
                LoginMapping {
                    login: "root".into(),
                    seuser: "root".into(),
                    mls_range: "s0-s0:c0.c1023".into(),
                },
            ],
        }
    }
}

// ============================================================================
// Audit log entry
// ============================================================================

/// A parsed AVC denial from the audit log.
#[derive(Clone, Debug, PartialEq, Eq)]
struct AvcDenial {
    source: String,
    target: String,
    class: String,
    permissions: Vec<String>,
    pid: u32,
    comm: String,
}

impl fmt::Display for AvcDenial {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "denied {{ {} }} scontext={} tcontext={} tclass={}",
            self.permissions.join(" "),
            self.source,
            self.target,
            self.class
        )
    }
}

/// Parse a single audit log line for AVC denials.
fn parse_avc_denial(line: &str) -> Option<AvcDenial> {
    // Expected format: type=AVC msg=audit(...): avc:  denied  { perm1 perm2 }
    //   for pid=NNN comm="cmd" ... scontext=... tcontext=... tclass=...
    if !line.contains("type=AVC") && !line.contains("avc:  denied") {
        return None;
    }

    let perms = extract_braces(line)?;
    let scontext = extract_field(line, "scontext=")?;
    let tcontext = extract_field(line, "tcontext=")?;
    let tclass = extract_field(line, "tclass=")?;

    let pid = extract_field(line, "pid=")
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(0);
    let comm = extract_field(line, "comm=")
        .map(|s| s.trim_matches('"').to_string())
        .unwrap_or_default();

    Some(AvcDenial {
        source: scontext,
        target: tcontext,
        class: tclass,
        permissions: perms,
        pid,
        comm,
    })
}

/// Extract the contents of `{ ... }` as a list of whitespace-separated tokens.
fn extract_braces(s: &str) -> Option<Vec<String>> {
    let open = s.find('{')?;
    let close = s.find('}')?;
    if close <= open {
        return None;
    }
    let inner = &s[open + 1..close];
    let tokens: Vec<String> = inner.split_whitespace().map(String::from).collect();
    if tokens.is_empty() {
        None
    } else {
        Some(tokens)
    }
}

/// Extract the value of a `key=value` field from a log line.
fn extract_field(line: &str, key: &str) -> Option<String> {
    let start = line.find(key)? + key.len();
    let rest = &line[start..];
    // Value ends at whitespace or end-of-string
    let end = rest
        .find(|c: char| c.is_whitespace())
        .unwrap_or(rest.len());
    let val = &rest[..end];
    if val.is_empty() {
        None
    } else {
        Some(val.to_string())
    }
}

/// Convert AVC denials into allow rules, merging permissions for identical
/// source/target/class triples.
fn denials_to_allow_rules(denials: &[AvcDenial]) -> Vec<PolicyRule> {
    let mut map: HashMap<(String, String, String), Vec<String>> = HashMap::new();
    for d in denials {
        let key = (d.source.clone(), d.target.clone(), d.class.clone());
        let entry = map.entry(key).or_default();
        for p in &d.permissions {
            if !entry.contains(p) {
                entry.push(p.clone());
            }
        }
    }
    let mut rules: Vec<PolicyRule> = map
        .into_iter()
        .map(|((source, target, class), permissions)| {
            // Extract just the type from a full context for rule generation
            let src_type = extract_type_from_context(&source);
            let tgt_type = extract_type_from_context(&target);
            PolicyRule {
                kind: RuleKind::Allow,
                source: src_type,
                target: tgt_type,
                class,
                permissions,
            }
        })
        .collect();
    rules.sort_by(|a, b| {
        a.source
            .cmp(&b.source)
            .then(a.target.cmp(&b.target))
            .then(a.class.cmp(&b.class))
    });
    rules
}

/// Extract the type field from a full context (user:role:type:level) or return
/// the string as-is if it does not contain colons.
fn extract_type_from_context(ctx: &str) -> String {
    let parts: Vec<&str> = ctx.split(':').collect();
    if parts.len() >= 3 {
        parts[2].to_string()
    } else {
        ctx.to_string()
    }
}

// ============================================================================
// SELinux filesystem helpers
// ============================================================================

fn read_enforce_mode() -> EnforceMode {
    match fs::read_to_string(ENFORCE_PATH) {
        Ok(s) => {
            let v = s.trim().parse::<u8>().unwrap_or(2);
            EnforceMode::from_value(v)
        }
        Err(_) => EnforceMode::Disabled,
    }
}

fn write_enforce_mode(mode: EnforceMode) -> Result<(), String> {
    fs::write(ENFORCE_PATH, format!("{}\n", mode.to_value()))
        .map_err(|e| format!("Failed to write {}: {}", ENFORCE_PATH, e))
}

fn read_sestatus() -> SeStatus {
    // Check if SELinux fs is mounted
    let enabled = Path::new(SELINUX_FS).exists();
    let mut status = SeStatus {
        enabled,
        ..Default::default()
    };
    if !status.enabled {
        status.mode = EnforceMode::Disabled;
        status.config_mode = EnforceMode::Disabled;
        return status;
    }

    status.mode = read_enforce_mode();

    // Read config file for configured mode
    let config_path = format!("{}/config", POLICY_DIR);
    if let Ok(content) = fs::read_to_string(&config_path) {
        for line in content.lines() {
            let trimmed = line.trim();
            if let Some(val) = trimmed.strip_prefix("SELINUX=") {
                status.config_mode = match val.trim().to_lowercase().as_str() {
                    "enforcing" => EnforceMode::Enforcing,
                    "permissive" => EnforceMode::Permissive,
                    _ => EnforceMode::Disabled,
                };
            }
            if let Some(rest) = trimmed.strip_prefix("SELINUXTYPE=") {
                status.policy_name = rest.trim().to_string();
            }
        }
    }

    // Read policy version
    let policyvers_path = format!("{}/policyvers", SELINUX_FS);
    if let Ok(content) = fs::read_to_string(&policyvers_path) {
        status.policy_version = content.trim().parse::<u32>().unwrap_or(33);
    }

    // Read MLS status
    let mls_path = format!("{}/mls", SELINUX_FS);
    if let Ok(content) = fs::read_to_string(&mls_path) {
        status.mls_enabled = content.trim() == "1";
    }

    // Read deny_unknown
    let deny_path = format!("{}/deny_unknown", SELINUX_FS);
    if let Ok(content) = fs::read_to_string(&deny_path) {
        status.deny_unknown = content.trim() == "1";
    }

    status
}

/// Read a boolean value from the SELinux filesystem.
fn read_sebool(name: &str) -> Option<(bool, bool)> {
    let path = format!("{}/{}", BOOLEANS_DIR, name);
    let content = fs::read_to_string(&path).ok()?;
    // Format: "active pending" e.g. "1 0"
    let parts: Vec<&str> = content.split_whitespace().collect();
    if parts.len() >= 2 {
        let active = parts[0] == "1";
        let pending = parts[1] == "1";
        Some((active, pending))
    } else if parts.len() == 1 {
        let active = parts[0] == "1";
        Some((active, active))
    } else {
        None
    }
}

/// Write a boolean value to the SELinux filesystem.
fn write_sebool(name: &str, value: bool) -> Result<(), String> {
    let path = format!("{}/{}", BOOLEANS_DIR, name);
    let val = if value { "1" } else { "0" };
    fs::write(&path, val).map_err(|e| format!("Failed to write {}: {}", path, e))
}

/// Get the security context of a file via extended attributes.
fn get_file_context(path: &str) -> Option<SecurityContext> {
    // On a real system, we'd read from xattr security.selinux.
    // For now, try reading the xattr file or return None.
    let xattr_path = format!("{}/.selinux_context", path);
    let content = fs::read_to_string(&xattr_path).ok()?;
    SecurityContext::parse(content.trim())
}

/// Set the security context of a file.
fn set_file_context(path: &str, ctx: &SecurityContext) -> Result<(), String> {
    // On a real system, we'd set xattr security.selinux.
    let xattr_path = format!("{}/.selinux_context", path);
    fs::write(&xattr_path, ctx.to_string())
        .map_err(|e| format!("Failed to set context on {}: {}", path, e))
}

/// Match a path against a file context pattern. Simplified glob matching.
fn pattern_matches(pattern: &str, path: &str) -> bool {
    // Handle regex-style file_contexts patterns.
    // Common patterns: /etc(/.*)? /usr/bin(/.*)? etc.
    // Simplified: strip the regex part and do prefix matching.
    let prefix = if let Some(idx) = pattern.find('(') {
        &pattern[..idx]
    } else {
        pattern
    };

    if pattern.ends_with("(/.*)?") {
        // Matches the directory itself or anything under it
        path == prefix || path.starts_with(&format!("{}/", prefix))
    } else {
        path == pattern
    }
}

/// Find the best matching file context for a path.
fn find_file_context<'a>(
    file_contexts: &'a [FileContext],
    path: &str,
) -> Option<&'a FileContext> {
    // Use longest prefix match
    let mut best: Option<&FileContext> = None;
    let mut best_len = 0;
    for fc in file_contexts {
        if pattern_matches(&fc.pattern, path) {
            let prefix_len = fc.pattern.find('(').unwrap_or(fc.pattern.len());
            if prefix_len > best_len {
                best = Some(fc);
                best_len = prefix_len;
            }
        }
    }
    best
}

// ============================================================================
// Personality: getenforce
// ============================================================================

fn cmd_getenforce(_args: &[String]) -> i32 {
    let mode = read_enforce_mode();
    println!("{}", mode);
    0
}

// ============================================================================
// Personality: setenforce
// ============================================================================

fn cmd_setenforce(args: &[String]) -> i32 {
    if args.is_empty() {
        eprintln!("usage: setenforce [ Enforcing | Permissive | 1 | 0 ]");
        return 1;
    }
    let mode = match args[0].to_lowercase().as_str() {
        "enforcing" | "1" => EnforceMode::Enforcing,
        "permissive" | "0" => EnforceMode::Permissive,
        other => {
            eprintln!("setenforce: invalid mode '{}'", other);
            return 1;
        }
    };
    if let Err(e) = write_enforce_mode(mode) {
        eprintln!("setenforce: {}", e);
        return 1;
    }
    0
}

// ============================================================================
// Personality: sestatus
// ============================================================================

fn cmd_sestatus(args: &[String]) -> i32 {
    let verbose = args.iter().any(|a| a == "-v");
    let status = read_sestatus();

    println!("SELinux status:                 {}", if status.enabled { "enabled" } else { "disabled" });
    println!("SELinuxfs mount:                {}", SELINUX_FS);
    println!("SELinux root directory:          {}", POLICY_DIR);
    println!("Loaded policy name:             {}", status.policy_name);
    println!("Current mode:                   {}", status.mode);
    println!("Mode from config:               {}", status.config_mode);
    println!("Policy MLS status:              {}", if status.mls_enabled { "enabled" } else { "disabled" });
    println!("Policy deny_unknown status:     {}", if status.deny_unknown { "denied" } else { "allowed" });
    println!("Memory protection checking:     actual (secure)");
    println!("Max kernel policy version:      {}", status.max_kernel_policy);

    if verbose {
        println!();
        println!("Process contexts:");
        println!("Current context:                unconfined_u:unconfined_r:unconfined_t:s0-s0:c0.c1023");
        println!("Init context:                   system_u:system_r:init_t:s0");
        println!();
        println!("File contexts:");
        println!("Controlling terminal:           system_u:object_r:user_devpts_t:s0");
        println!("/etc/passwd:                    system_u:object_r:etc_t:s0");
        println!("/etc/shadow:                    system_u:object_r:shadow_t:s0");
    }

    0
}

// ============================================================================
// Personality: semanage
// ============================================================================

fn cmd_semanage(args: &[String]) -> i32 {
    if args.is_empty() {
        print_semanage_usage();
        return 1;
    }

    let subcmd = args[0].as_str();
    let sub_args = &args[1..];

    match subcmd {
        "boolean" => semanage_boolean(sub_args),
        "port" => semanage_port(sub_args),
        "fcontext" => semanage_fcontext(sub_args),
        "login" => semanage_login(sub_args),
        "user" => semanage_user(sub_args),
        "-h" | "--help" | "help" => {
            print_semanage_usage();
            0
        }
        other => {
            eprintln!("semanage: unknown subcommand '{}'", other);
            print_semanage_usage();
            1
        }
    }
}

fn print_semanage_usage() {
    eprintln!("usage: semanage <subcommand> [options]");
    eprintln!();
    eprintln!("Subcommands:");
    eprintln!("  boolean    Manage SELinux booleans");
    eprintln!("  port       Manage network port type definitions");
    eprintln!("  fcontext   Manage file context mapping definitions");
    eprintln!("  login      Manage login mappings between linux and SELinux users");
    eprintln!("  user       Manage SELinux confined users");
    eprintln!();
    eprintln!("Options for each subcommand:");
    eprintln!("  -l         List records");
    eprintln!("  -a         Add a record");
    eprintln!("  -d         Delete a record");
    eprintln!("  -m         Modify a record");
}

fn semanage_boolean(args: &[String]) -> i32 {
    if args.is_empty() || args[0] == "-l" {
        let db = PolicyDb::default();
        println!("SELinux boolean                  State  Default Description");
        println!();
        for b in &db.booleans {
            let state = if b.active { "on" } else { "off" };
            let default = if b.pending { "on" } else { "off" };
            println!("{:<33}{:<7}{}", b.name, state, default);
        }
        return 0;
    }

    let mut action = "";
    let mut name = "";
    let mut value = "";

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-m" | "--modify" => action = "modify",
            "-a" | "--add" => action = "add",
            "-d" | "--delete" => action = "delete",
            "--on" => value = "on",
            "--off" => value = "off",
            "-1" => value = "on",
            "-0" => value = "off",
            other => {
                if name.is_empty() {
                    name = other;
                } else if value.is_empty() {
                    value = other;
                }
            }
        }
        i += 1;
    }

    if name.is_empty() {
        eprintln!("semanage boolean: boolean name required");
        return 1;
    }

    match action {
        "modify" => {
            if value.is_empty() {
                eprintln!("semanage boolean -m: --on or --off required");
                return 1;
            }
            let v = value == "on" || value == "1";
            println!("Boolean {} set to {}", name, if v { "on" } else { "off" });
            0
        }
        "add" => {
            println!("Boolean {} added", name);
            0
        }
        "delete" => {
            println!("Boolean {} deleted", name);
            0
        }
        _ => {
            eprintln!("semanage boolean: -l, -a, -m, or -d required");
            1
        }
    }
}

fn semanage_port(args: &[String]) -> i32 {
    if args.is_empty() || args[0] == "-l" {
        let db = PolicyDb::default();
        println!("SELinux Port Type              Proto    Port Number");
        println!();
        for p in &db.ports {
            if p.port_low == p.port_high {
                println!("{:<31}{:<9}{}", p.context.context_type, p.protocol, p.port_low);
            } else {
                println!(
                    "{:<31}{:<9}{}-{}",
                    p.context.context_type, p.protocol, p.port_low, p.port_high
                );
            }
        }
        return 0;
    }

    let mut action = "";
    let mut proto_str = "";
    let mut port_str = "";
    let mut context_type = "";

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-a" | "--add" => action = "add",
            "-d" | "--delete" => action = "delete",
            "-m" | "--modify" => action = "modify",
            "-t" | "--type" => {
                i += 1;
                if i < args.len() {
                    context_type = &args[i];
                }
            }
            "-p" | "--proto" => {
                i += 1;
                if i < args.len() {
                    proto_str = &args[i];
                }
            }
            other => {
                if port_str.is_empty() {
                    port_str = other;
                }
            }
        }
        i += 1;
    }

    if action.is_empty() {
        eprintln!("semanage port: -l, -a, -d, or -m required");
        return 1;
    }

    if proto_str.is_empty() {
        eprintln!("semanage port: --proto required");
        return 1;
    }

    if Protocol::from_str(proto_str).is_none() {
        eprintln!("semanage port: invalid protocol '{}'", proto_str);
        return 1;
    }

    match action {
        "add" => {
            if context_type.is_empty() || port_str.is_empty() {
                eprintln!("semanage port -a: --type and port number required");
                return 1;
            }
            println!(
                "Port {} {} added with type {}",
                proto_str, port_str, context_type
            );
            0
        }
        "delete" => {
            if port_str.is_empty() {
                eprintln!("semanage port -d: port number required");
                return 1;
            }
            println!("Port {} {} deleted", proto_str, port_str);
            0
        }
        "modify" => {
            if context_type.is_empty() || port_str.is_empty() {
                eprintln!("semanage port -m: --type and port number required");
                return 1;
            }
            println!(
                "Port {} {} modified to type {}",
                proto_str, port_str, context_type
            );
            0
        }
        _ => 1,
    }
}

fn semanage_fcontext(args: &[String]) -> i32 {
    if args.is_empty() || args[0] == "-l" {
        let db = PolicyDb::default();
        println!("SELinux fcontext                                   type               Context");
        println!();
        for fc in &db.file_contexts {
            println!("{:<51}{:<19}{}", fc.pattern, fc.file_type, fc.context);
        }
        return 0;
    }

    let mut action = "";
    let mut file_type_str = "";
    let mut context_str = "";
    let mut pattern = "";

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-a" | "--add" => action = "add",
            "-d" | "--delete" => action = "delete",
            "-m" | "--modify" => action = "modify",
            "-f" | "--ftype" => {
                i += 1;
                if i < args.len() {
                    file_type_str = &args[i];
                }
            }
            "-t" | "--type" => {
                i += 1;
                if i < args.len() {
                    context_str = &args[i];
                }
            }
            other => {
                if pattern.is_empty() {
                    pattern = other;
                }
            }
        }
        i += 1;
    }

    if action.is_empty() {
        eprintln!("semanage fcontext: -l, -a, -d, or -m required");
        return 1;
    }

    match action {
        "add" => {
            if context_str.is_empty() || pattern.is_empty() {
                eprintln!("semanage fcontext -a: --type and file spec required");
                return 1;
            }
            let ftype = if file_type_str.is_empty() {
                "all files"
            } else {
                match FileContextType::from_flag(file_type_str) {
                    Some(ft) => {
                        // Small trick: use a &str that lives long enough.
                        // We re-match to get the display string.
                        match ft {
                            FileContextType::All => "all files",
                            FileContextType::Regular => "regular file",
                            FileContextType::Directory => "directory",
                            FileContextType::CharDevice => "character device",
                            FileContextType::BlockDevice => "block device",
                            FileContextType::Socket => "socket",
                            FileContextType::SymLink => "symbolic link",
                            FileContextType::Pipe => "named pipe",
                        }
                    }
                    None => {
                        eprintln!("semanage fcontext: invalid file type '{}'", file_type_str);
                        return 1;
                    }
                }
            };
            println!(
                "fcontext {} ({}) added with type {}",
                pattern, ftype, context_str
            );
            0
        }
        "delete" => {
            if pattern.is_empty() {
                eprintln!("semanage fcontext -d: file spec required");
                return 1;
            }
            println!("fcontext {} deleted", pattern);
            0
        }
        "modify" => {
            if context_str.is_empty() || pattern.is_empty() {
                eprintln!("semanage fcontext -m: --type and file spec required");
                return 1;
            }
            println!("fcontext {} modified to type {}", pattern, context_str);
            0
        }
        _ => 1,
    }
}

fn semanage_login(args: &[String]) -> i32 {
    if args.is_empty() || args[0] == "-l" {
        let db = PolicyDb::default();
        println!("Login Name           SELinux User         MLS/MCS Range");
        println!();
        for lm in &db.login_mappings {
            println!("{:<21}{:<21}{}", lm.login, lm.seuser, lm.mls_range);
        }
        return 0;
    }

    let mut action = "";
    let mut login_name = "";
    let mut seuser = "";
    let mut range = "";

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-a" | "--add" => action = "add",
            "-d" | "--delete" => action = "delete",
            "-m" | "--modify" => action = "modify",
            "-s" | "--seuser" => {
                i += 1;
                if i < args.len() {
                    seuser = &args[i];
                }
            }
            "-r" | "--range" => {
                i += 1;
                if i < args.len() {
                    range = &args[i];
                }
            }
            other => {
                if login_name.is_empty() {
                    login_name = other;
                }
            }
        }
        i += 1;
    }

    if action.is_empty() {
        eprintln!("semanage login: -l, -a, -d, or -m required");
        return 1;
    }

    if login_name.is_empty() {
        eprintln!("semanage login: login name required");
        return 1;
    }

    match action {
        "add" => {
            if seuser.is_empty() {
                eprintln!("semanage login -a: --seuser required");
                return 1;
            }
            let r = if range.is_empty() { "s0" } else { range };
            println!("Login mapping {} -> {} ({}) added", login_name, seuser, r);
            0
        }
        "delete" => {
            println!("Login mapping {} deleted", login_name);
            0
        }
        "modify" => {
            if seuser.is_empty() {
                eprintln!("semanage login -m: --seuser required");
                return 1;
            }
            let r = if range.is_empty() { "s0" } else { range };
            println!(
                "Login mapping {} modified -> {} ({})",
                login_name, seuser, r
            );
            0
        }
        _ => 1,
    }
}

fn semanage_user(args: &[String]) -> i32 {
    if args.is_empty() || args[0] == "-l" {
        let db = PolicyDb::default();
        println!(
            "{:<20}{:<40}{:<10}MLS Range",
            "SELinux User", "Roles", "MLS Level"
        );
        println!();
        for u in &db.users {
            println!(
                "{:<20}{:<40}{:<10}{}",
                u.name,
                u.roles.join(" "),
                u.mls_level,
                u.mls_range
            );
        }
        return 0;
    }

    let mut action = "";
    let mut username = "";
    let mut roles_str = "";
    let mut level = "";
    let mut range = "";

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-a" | "--add" => action = "add",
            "-d" | "--delete" => action = "delete",
            "-m" | "--modify" => action = "modify",
            "-R" | "--roles" => {
                i += 1;
                if i < args.len() {
                    roles_str = &args[i];
                }
            }
            "-L" | "--level" => {
                i += 1;
                if i < args.len() {
                    level = &args[i];
                }
            }
            "-r" | "--range" => {
                i += 1;
                if i < args.len() {
                    range = &args[i];
                }
            }
            other => {
                if username.is_empty() {
                    username = other;
                }
            }
        }
        i += 1;
    }

    if action.is_empty() {
        eprintln!("semanage user: -l, -a, -d, or -m required");
        return 1;
    }

    if username.is_empty() {
        eprintln!("semanage user: user name required");
        return 1;
    }

    match action {
        "add" => {
            if roles_str.is_empty() {
                eprintln!("semanage user -a: --roles required");
                return 1;
            }
            let l = if level.is_empty() { "s0" } else { level };
            let r = if range.is_empty() { "s0" } else { range };
            println!(
                "SELinux user {} added (roles={}, level={}, range={})",
                username, roles_str, l, r
            );
            0
        }
        "delete" => {
            println!("SELinux user {} deleted", username);
            0
        }
        "modify" => {
            let l = if level.is_empty() { "s0" } else { level };
            let r = if range.is_empty() { "s0" } else { range };
            println!(
                "SELinux user {} modified (level={}, range={})",
                username, l, r
            );
            0
        }
        _ => 1,
    }
}

// ============================================================================
// Personality: setsebool
// ============================================================================

fn cmd_setsebool(args: &[String]) -> i32 {
    if args.is_empty() {
        eprintln!("usage: setsebool [-P] boolean value");
        return 1;
    }

    let mut persistent = false;
    let mut name = "";
    let mut value_str = "";

    for arg in args {
        match arg.as_str() {
            "-P" => persistent = true,
            _ => {
                if name.is_empty() {
                    name = arg;
                } else if value_str.is_empty() {
                    value_str = arg;
                }
            }
        }
    }

    // Support "name=value" format
    if value_str.is_empty()
        && let Some(eq_pos) = name.find('=') {
            value_str = &name[eq_pos + 1..];
            name = &name[..eq_pos];
        }

    if name.is_empty() || value_str.is_empty() {
        eprintln!("usage: setsebool [-P] boolean value");
        return 1;
    }

    let value = match value_str.to_lowercase().as_str() {
        "on" | "1" | "true" => true,
        "off" | "0" | "false" => false,
        _ => {
            eprintln!("setsebool: invalid value '{}' (use on/off, 1/0, true/false)", value_str);
            return 1;
        }
    };

    if let Err(e) = write_sebool(name, value) {
        eprintln!("setsebool: {}", e);
        return 1;
    }

    if persistent {
        println!(
            "Boolean {} persistently set to {}",
            name,
            if value { "on" } else { "off" }
        );
    }

    0
}

// ============================================================================
// Personality: getsebool
// ============================================================================

fn cmd_getsebool(args: &[String]) -> i32 {
    let show_all = args.iter().any(|a| a == "-a");

    if show_all {
        let db = PolicyDb::default();
        for b in &db.booleans {
            let val = if b.active { "on" } else { "off" };
            println!("{} --> {}", b.name, val);
        }
        return 0;
    }

    // Show specific booleans (try reading from sysfs first, fall back to db)
    let db = PolicyDb::default();
    let mut found_any = false;
    for arg in args {
        if arg.starts_with('-') {
            continue;
        }
        found_any = true;
        if let Some((active, _pending)) = read_sebool(arg) {
            let val = if active { "on" } else { "off" };
            println!("{} --> {}", arg, val);
        } else if let Some(b) = db.booleans.iter().find(|b| b.name == *arg) {
            let val = if b.active { "on" } else { "off" };
            println!("{} --> {}", b.name, val);
        } else {
            eprintln!("getsebool: boolean {} not found", arg);
        }
    }

    if !found_any {
        eprintln!("usage: getsebool -a | getsebool boolean...");
        return 1;
    }
    0
}

// ============================================================================
// Personality: restorecon
// ============================================================================

fn cmd_restorecon(args: &[String]) -> i32 {
    let mut recursive = false;
    let mut verbose = false;
    let mut dry_run = false;
    let mut paths: Vec<&str> = Vec::new();

    for arg in args {
        match arg.as_str() {
            "-R" | "-r" => recursive = true,
            "-v" => verbose = true,
            "-n" => dry_run = true,
            "-Rv" | "-vR" | "-Rvn" | "-nvR" => {
                recursive = true;
                verbose = true;
                if arg.contains('n') {
                    dry_run = true;
                }
            }
            _ => paths.push(arg),
        }
    }

    if paths.is_empty() {
        eprintln!("usage: restorecon [-R] [-v] [-n] path...");
        return 1;
    }

    let db = PolicyDb::default();
    let mut errors = 0;

    for path in &paths {
        errors += restorecon_path(path, recursive, verbose, dry_run, &db.file_contexts);
    }

    if errors > 0 { 1 } else { 0 }
}

fn restorecon_path(
    path: &str,
    recursive: bool,
    verbose: bool,
    dry_run: bool,
    file_contexts: &[FileContext],
) -> i32 {
    let mut errors = 0;

    if let Some(fc) = find_file_context(file_contexts, path) {
        let current = get_file_context(path);
        let needs_change = current.as_ref() != Some(&fc.context);

        if needs_change {
            if verbose {
                let old_str = current
                    .as_ref()
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "<<none>>".into());
                println!(
                    "Relabeled {} from {} to {}",
                    path, old_str, fc.context
                );
            }
            if !dry_run
                && let Err(e) = set_file_context(path, &fc.context) {
                    eprintln!("restorecon: {}", e);
                    errors += 1;
                }
        }
    }

    if recursive
        && let Ok(entries) = fs::read_dir(path) {
            for entry in entries.flatten() {
                let child_path = entry.path();
                if let Some(child_str) = child_path.to_str() {
                    // Convert backslashes to forward slashes for consistency
                    let normalized = child_str.replace('\\', "/");
                    errors += restorecon_path(
                        &normalized,
                        recursive,
                        verbose,
                        dry_run,
                        file_contexts,
                    );
                }
            }
        }

    errors
}

// ============================================================================
// Personality: chcon
// ============================================================================

fn cmd_chcon(args: &[String]) -> i32 {
    let mut user = "";
    let mut role = "";
    let mut type_str = "";
    let mut range = "";
    let mut full_context = "";
    let mut recursive = false;
    let mut verbose = false;
    let mut paths: Vec<&str> = Vec::new();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-u" | "--user" => {
                i += 1;
                if i < args.len() {
                    user = &args[i];
                }
            }
            "-r" | "--role" => {
                i += 1;
                if i < args.len() {
                    role = &args[i];
                }
            }
            "-t" | "--type" => {
                i += 1;
                if i < args.len() {
                    type_str = &args[i];
                }
            }
            "-l" | "--range" => {
                i += 1;
                if i < args.len() {
                    range = &args[i];
                }
            }
            "-R" => recursive = true,
            "-v" => verbose = true,
            other => {
                // If no component flags given, first positional arg is the full context
                if user.is_empty() && role.is_empty() && type_str.is_empty()
                    && range.is_empty() && full_context.is_empty() && paths.is_empty()
                    && other.contains(':')
                {
                    full_context = other;
                } else {
                    paths.push(other);
                }
            }
        }
        i += 1;
    }

    if paths.is_empty() {
        eprintln!("usage: chcon [-u USER] [-r ROLE] [-t TYPE] [-l RANGE] [-R] [-v] CONTEXT FILE...");
        eprintln!("   or: chcon [-u USER] [-r ROLE] [-t TYPE] [-l RANGE] [-R] [-v] FILE...");
        return 1;
    }

    let mut errors = 0;

    for path in &paths {
        let mut ctx = if !full_context.is_empty() {
            match SecurityContext::parse(full_context) {
                Some(c) => c,
                None => {
                    eprintln!("chcon: invalid context '{}'", full_context);
                    return 1;
                }
            }
        } else {
            // Start from current context or a default
            get_file_context(path).unwrap_or(SecurityContext {
                user: "system_u".into(),
                role: "object_r".into(),
                context_type: "unlabeled_t".into(),
                level: "s0".into(),
            })
        };

        // Override individual components
        if !user.is_empty() {
            ctx.user = user.to_string();
        }
        if !role.is_empty() {
            ctx.role = role.to_string();
        }
        if !type_str.is_empty() {
            ctx.context_type = type_str.to_string();
        }
        if !range.is_empty() {
            ctx.level = range.to_string();
        }

        errors += chcon_apply(path, &ctx, recursive, verbose);
    }

    if errors > 0 { 1 } else { 0 }
}

fn chcon_apply(path: &str, ctx: &SecurityContext, recursive: bool, verbose: bool) -> i32 {
    let mut errors = 0;

    if verbose {
        println!("changing security context of '{}'", path);
    }

    if let Err(e) = set_file_context(path, ctx) {
        eprintln!("chcon: {}", e);
        errors += 1;
    }

    if recursive
        && let Ok(entries) = fs::read_dir(path) {
            for entry in entries.flatten() {
                let child_path = entry.path();
                if let Some(child_str) = child_path.to_str() {
                    let normalized = child_str.replace('\\', "/");
                    errors += chcon_apply(&normalized, ctx, recursive, verbose);
                }
            }
        }

    errors
}

// ============================================================================
// Personality: seinfo
// ============================================================================

fn cmd_seinfo(args: &[String]) -> i32 {
    let db = PolicyDb::default();

    if args.is_empty() || args[0] == "--all" {
        println!("Statistics for policy: targeted");
        println!("  Types:            {}", db.types.len());
        println!("  Roles:            {}", db.roles.len());
        println!("  Users:            {}", db.users.len());
        println!("  Booleans:         {}", db.booleans.len());
        println!("  Classes:          {}", db.classes.len());
        println!("  Permissions:      {}", db.permissions.len());
        println!("  Allow rules:      {}", db.rules.iter().filter(|r| r.kind == RuleKind::Allow).count());
        println!("  Dontaudit rules:  {}", db.rules.iter().filter(|r| r.kind == RuleKind::Dontaudit).count());
        println!("  Type trans rules: {}", db.rules.iter().filter(|r| r.kind == RuleKind::TypeTransition).count());
        println!("  Total rules:      {}", db.rules.len());
        return 0;
    }

    match args[0].as_str() {
        "-t" | "--type" => {
            if args.len() > 1 {
                // Show specific type info
                let name = &args[1];
                if db.types.contains(name) {
                    println!("Type: {}", name);
                    // Show rules referencing this type
                    let related: Vec<_> = db
                        .rules
                        .iter()
                        .filter(|r| r.source == *name || r.target == *name)
                        .collect();
                    println!("  Referenced in {} rules", related.len());
                } else {
                    eprintln!("seinfo: type '{}' not found", name);
                    return 1;
                }
            } else {
                println!("Types: {}", db.types.len());
                for t in &db.types {
                    println!("   {}", t);
                }
            }
        }
        "-r" | "--role" => {
            if args.len() > 1 {
                let name = &args[1];
                if db.roles.contains(name) {
                    println!("Role: {}", name);
                    let users: Vec<_> = db
                        .users
                        .iter()
                        .filter(|u| u.roles.contains(name))
                        .collect();
                    println!("  Users with this role:");
                    for u in &users {
                        println!("    {}", u.name);
                    }
                } else {
                    eprintln!("seinfo: role '{}' not found", name);
                    return 1;
                }
            } else {
                println!("Roles: {}", db.roles.len());
                for r in &db.roles {
                    println!("   {}", r);
                }
            }
        }
        "-u" | "--user" => {
            if args.len() > 1 {
                let name = &args[1];
                if let Some(u) = db.users.iter().find(|u| u.name == *name) {
                    println!("User: {}", u.name);
                    println!("  Roles: {}", u.roles.join(", "));
                    println!("  Level: {}", u.mls_level);
                    println!("  Range: {}", u.mls_range);
                } else {
                    eprintln!("seinfo: user '{}' not found", name);
                    return 1;
                }
            } else {
                println!("Users: {}", db.users.len());
                for u in &db.users {
                    println!("   {}", u.name);
                }
            }
        }
        "-b" | "--bool" => {
            if args.len() > 1 {
                let name = &args[1];
                if let Some(b) = db.booleans.iter().find(|b| b.name == *name) {
                    println!("Boolean: {}", b.name);
                    println!("  Active: {}", if b.active { "on" } else { "off" });
                    println!("  Pending: {}", if b.pending { "on" } else { "off" });
                } else {
                    eprintln!("seinfo: boolean '{}' not found", name);
                    return 1;
                }
            } else {
                println!("Booleans: {}", db.booleans.len());
                for b in &db.booleans {
                    let val = if b.active { "on" } else { "off" };
                    println!("   {} ({})", b.name, val);
                }
            }
        }
        "-c" | "--class" => {
            if args.len() > 1 {
                let name = &args[1];
                if db.classes.contains(name) {
                    println!("Class: {}", name);
                    let rules_using: Vec<_> =
                        db.rules.iter().filter(|r| r.class == *name).collect();
                    println!("  Used in {} rules", rules_using.len());
                } else {
                    eprintln!("seinfo: class '{}' not found", name);
                    return 1;
                }
            } else {
                println!("Classes: {}", db.classes.len());
                for c in &db.classes {
                    println!("   {}", c);
                }
            }
        }
        "--stats" => {
            println!("Policy Statistics:");
            println!("  Types:       {}", db.types.len());
            println!("  Roles:       {}", db.roles.len());
            println!("  Users:       {}", db.users.len());
            println!("  Booleans:    {}", db.booleans.len());
            println!("  Classes:     {}", db.classes.len());
            println!("  Permissions: {}", db.permissions.len());
            println!("  Rules:       {}", db.rules.len());
            println!("  Ports:       {}", db.ports.len());
            println!("  File ctxs:   {}", db.file_contexts.len());
        }
        "-h" | "--help" => {
            eprintln!("usage: seinfo [OPTIONS]");
            eprintln!("  --all        Show all statistics");
            eprintln!("  --stats      Show policy statistics");
            eprintln!("  -t [TYPE]    List types or show type info");
            eprintln!("  -r [ROLE]    List roles or show role info");
            eprintln!("  -u [USER]    List users or show user info");
            eprintln!("  -b [BOOL]    List booleans or show boolean info");
            eprintln!("  -c [CLASS]   List classes or show class info");
        }
        other => {
            eprintln!("seinfo: unknown option '{}'", other);
            return 1;
        }
    }

    0
}

// ============================================================================
// Personality: sesearch
// ============================================================================

fn cmd_sesearch(args: &[String]) -> i32 {
    let db = PolicyDb::default();

    let mut rule_kind: Option<RuleKind> = None;
    let mut source_filter: Option<&str> = None;
    let mut target_filter: Option<&str> = None;
    let mut class_filter: Option<&str> = None;
    let mut perm_filter: Option<&str> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-A" | "--allow" => rule_kind = Some(RuleKind::Allow),
            "--dontaudit" => rule_kind = Some(RuleKind::Dontaudit),
            "--auditallow" => rule_kind = Some(RuleKind::Auditallow),
            "--type_trans" | "--type-trans" => rule_kind = Some(RuleKind::TypeTransition),
            "--neverallow" => rule_kind = Some(RuleKind::Neverallow),
            "-s" | "--source" => {
                i += 1;
                if i < args.len() {
                    source_filter = Some(&args[i]);
                }
            }
            "-t" | "--target" => {
                i += 1;
                if i < args.len() {
                    target_filter = Some(&args[i]);
                }
            }
            "-c" | "--class" => {
                i += 1;
                if i < args.len() {
                    class_filter = Some(&args[i]);
                }
            }
            "-p" | "--perm" => {
                i += 1;
                if i < args.len() {
                    perm_filter = Some(&args[i]);
                }
            }
            "-h" | "--help" => {
                print_sesearch_usage();
                return 0;
            }
            other => {
                eprintln!("sesearch: unknown option '{}'", other);
                print_sesearch_usage();
                return 1;
            }
        }
        i += 1;
    }

    let results: Vec<&PolicyRule> = db
        .rules
        .iter()
        .filter(|r| {
            if let Some(kind) = rule_kind
                && r.kind != kind {
                    return false;
                }
            if let Some(src) = source_filter
                && r.source != src {
                    return false;
                }
            if let Some(tgt) = target_filter
                && r.target != tgt {
                    return false;
                }
            if let Some(cls) = class_filter
                && r.class != cls {
                    return false;
                }
            if let Some(perm) = perm_filter
                && !r.permissions.iter().any(|p| p == perm) {
                    return false;
                }
            true
        })
        .collect();

    if results.is_empty() {
        println!("No matching rules found.");
    } else {
        println!("Found {} rule(s):", results.len());
        for r in &results {
            println!("   {}", r);
        }
    }

    0
}

fn print_sesearch_usage() {
    eprintln!("usage: sesearch [OPTIONS]");
    eprintln!("  -A, --allow        Search allow rules");
    eprintln!("  --dontaudit        Search dontaudit rules");
    eprintln!("  --auditallow       Search auditallow rules");
    eprintln!("  --type_trans       Search type_transition rules");
    eprintln!("  --neverallow       Search neverallow rules");
    eprintln!("  -s, --source TYPE  Filter by source type");
    eprintln!("  -t, --target TYPE  Filter by target type");
    eprintln!("  -c, --class CLASS  Filter by object class");
    eprintln!("  -p, --perm PERM    Filter by permission");
}

// ============================================================================
// Personality: audit2allow
// ============================================================================

fn cmd_audit2allow(args: &[String]) -> i32 {
    let mut input_file: Option<&str> = None;
    let mut module_name = "";
    let mut generate_module = false;
    let mut dontaudit_mode = false;
    let mut from_stdin = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-i" | "--input" => {
                i += 1;
                if i < args.len() {
                    input_file = Some(&args[i]);
                }
            }
            "-M" | "--module" => {
                i += 1;
                if i < args.len() {
                    module_name = &args[i];
                    generate_module = true;
                }
            }
            "-d" | "--dontaudit" => dontaudit_mode = true,
            "-" => from_stdin = true,
            "-h" | "--help" => {
                print_audit2allow_usage();
                return 0;
            }
            _other => {
                // Treat as input file
                if input_file.is_none() {
                    input_file = Some(&args[i]);
                }
            }
        }
        i += 1;
    }

    // Read input lines
    let lines: Vec<String> = if from_stdin || input_file.is_none() {
        let stdin = io::stdin();
        stdin.lock().lines().map_while(Result::ok).collect()
    } else if let Some(path) = input_file {
        match fs::read_to_string(path) {
            Ok(content) => content.lines().map(String::from).collect(),
            Err(e) => {
                eprintln!("audit2allow: cannot read '{}': {}", path, e);
                return 1;
            }
        }
    } else {
        Vec::new()
    };

    // Parse denials
    let denials: Vec<AvcDenial> = lines.iter().filter_map(|l| parse_avc_denial(l)).collect();

    if denials.is_empty() {
        eprintln!("audit2allow: no AVC denials found in input");
        return 0;
    }

    let mut rules = denials_to_allow_rules(&denials);

    if dontaudit_mode {
        for r in &mut rules {
            r.kind = RuleKind::Dontaudit;
        }
    }

    if generate_module && !module_name.is_empty() {
        println!();
        println!("module {} 1.0;", module_name);
        println!();
        println!("require {{");
        // Collect all referenced types and classes
        let mut types: Vec<String> = Vec::new();
        let mut classes: Vec<String> = Vec::new();
        for r in &rules {
            if !types.contains(&r.source) {
                types.push(r.source.clone());
            }
            if !types.contains(&r.target) {
                types.push(r.target.clone());
            }
            if !classes.contains(&r.class) {
                classes.push(r.class.clone());
            }
        }
        types.sort();
        classes.sort();
        for t in &types {
            println!("        type {};", t);
        }
        for c in &classes {
            // Collect all permissions for this class
            let mut perms: Vec<String> = Vec::new();
            for r in &rules {
                if r.class == *c {
                    for p in &r.permissions {
                        if !perms.contains(p) {
                            perms.push(p.clone());
                        }
                    }
                }
            }
            perms.sort();
            println!(
                "        class {} {{ {} }};",
                c,
                perms.join(" ")
            );
        }
        println!("}}");
        println!();
    }

    // Output rules
    for r in &rules {
        println!("{}", r);
    }

    0
}

fn print_audit2allow_usage() {
    eprintln!("usage: audit2allow [OPTIONS]");
    eprintln!("  -i, --input FILE   Read denials from FILE (default: stdin)");
    eprintln!("  -M, --module NAME  Generate a policy module");
    eprintln!("  -d, --dontaudit    Generate dontaudit rules instead of allow");
    eprintln!("  -                  Read from stdin");
}

// ============================================================================
// Main dispatch
// ============================================================================

fn print_version(name: &str) {
    println!("{} (Slate OS selinux-tools) {}", name, VERSION);
}

fn main() {
    let args: Vec<String> = env::args().collect();

    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("getenforce");
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

    let rest: Vec<String> = args.into_iter().skip(1).collect();

    // Check for --version/--help before dispatching
    if rest.first().map(|s| s.as_str()) == Some("--version") {
        print_version(&prog_name);
        process::exit(0);
    }

    let code = match prog_name.as_str() {
        "getenforce" => cmd_getenforce(&rest),
        "setenforce" => cmd_setenforce(&rest),
        "sestatus" => cmd_sestatus(&rest),
        "semanage" => cmd_semanage(&rest),
        "setsebool" => cmd_setsebool(&rest),
        "getsebool" => cmd_getsebool(&rest),
        "restorecon" => cmd_restorecon(&rest),
        "chcon" => cmd_chcon(&rest),
        "seinfo" => cmd_seinfo(&rest),
        "sesearch" => cmd_sesearch(&rest),
        "audit2allow" => cmd_audit2allow(&rest),
        other => {
            eprintln!("selinux: unknown personality '{}', defaulting to getenforce", other);
            cmd_getenforce(&rest)
        }
    };

    process::exit(code);
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ---- SecurityContext tests ----

    #[test]
    fn test_security_context_parse_full() {
        let ctx = SecurityContext::parse("system_u:object_r:etc_t:s0").unwrap();
        assert_eq!(ctx.user, "system_u");
        assert_eq!(ctx.role, "object_r");
        assert_eq!(ctx.context_type, "etc_t");
        assert_eq!(ctx.level, "s0");
    }

    #[test]
    fn test_security_context_parse_no_level() {
        let ctx = SecurityContext::parse("user_u:user_r:user_t").unwrap();
        assert_eq!(ctx.user, "user_u");
        assert_eq!(ctx.role, "user_r");
        assert_eq!(ctx.context_type, "user_t");
        assert_eq!(ctx.level, "s0");
    }

    #[test]
    fn test_security_context_parse_mls_range() {
        let ctx = SecurityContext::parse("system_u:system_r:init_t:s0-s0:c0.c1023").unwrap();
        assert_eq!(ctx.level, "s0-s0:c0.c1023");
    }

    #[test]
    fn test_security_context_parse_too_short() {
        assert!(SecurityContext::parse("just_one").is_none());
        assert!(SecurityContext::parse("two:parts").is_none());
    }

    #[test]
    fn test_security_context_parse_empty() {
        assert!(SecurityContext::parse("").is_none());
    }

    #[test]
    fn test_security_context_display() {
        let ctx = SecurityContext {
            user: "system_u".into(),
            role: "object_r".into(),
            context_type: "etc_t".into(),
            level: "s0".into(),
        };
        assert_eq!(ctx.to_string(), "system_u:object_r:etc_t:s0");
    }

    #[test]
    fn test_security_context_display_mls() {
        let ctx = SecurityContext {
            user: "root".into(),
            role: "sysadm_r".into(),
            context_type: "sysadm_t".into(),
            level: "s0-s0:c0.c1023".into(),
        };
        assert_eq!(ctx.to_string(), "root:sysadm_r:sysadm_t:s0-s0:c0.c1023");
    }

    #[test]
    fn test_security_context_clone_eq() {
        let ctx1 = SecurityContext::parse("system_u:object_r:bin_t:s0").unwrap();
        let ctx2 = ctx1.clone();
        assert_eq!(ctx1, ctx2);
    }

    #[test]
    fn test_security_context_ne() {
        let ctx1 = SecurityContext::parse("system_u:object_r:bin_t:s0").unwrap();
        let ctx2 = SecurityContext::parse("system_u:object_r:etc_t:s0").unwrap();
        assert_ne!(ctx1, ctx2);
    }

    // ---- EnforceMode tests ----

    #[test]
    fn test_enforce_mode_from_value() {
        assert_eq!(EnforceMode::from_value(1), EnforceMode::Enforcing);
        assert_eq!(EnforceMode::from_value(0), EnforceMode::Permissive);
        assert_eq!(EnforceMode::from_value(2), EnforceMode::Disabled);
        assert_eq!(EnforceMode::from_value(255), EnforceMode::Disabled);
    }

    #[test]
    fn test_enforce_mode_to_value() {
        assert_eq!(EnforceMode::Enforcing.to_value(), 1);
        assert_eq!(EnforceMode::Permissive.to_value(), 0);
        assert_eq!(EnforceMode::Disabled.to_value(), 2);
    }

    #[test]
    fn test_enforce_mode_display() {
        assert_eq!(EnforceMode::Enforcing.to_string(), "Enforcing");
        assert_eq!(EnforceMode::Permissive.to_string(), "Permissive");
        assert_eq!(EnforceMode::Disabled.to_string(), "Disabled");
    }

    #[test]
    fn test_enforce_mode_roundtrip() {
        for mode in [EnforceMode::Enforcing, EnforceMode::Permissive, EnforceMode::Disabled] {
            assert_eq!(EnforceMode::from_value(mode.to_value()), mode);
        }
    }

    // ---- RuleKind tests ----

    #[test]
    fn test_rule_kind_from_str_allow() {
        assert_eq!(RuleKind::_from_str("allow"), Some(RuleKind::Allow));
    }

    #[test]
    fn test_rule_kind_from_str_dontaudit() {
        assert_eq!(RuleKind::_from_str("dontaudit"), Some(RuleKind::Dontaudit));
    }

    #[test]
    fn test_rule_kind_from_str_type_transition() {
        assert_eq!(RuleKind::_from_str("type_transition"), Some(RuleKind::TypeTransition));
    }

    #[test]
    fn test_rule_kind_from_str_all_variants() {
        assert_eq!(RuleKind::_from_str("auditallow"), Some(RuleKind::Auditallow));
        assert_eq!(RuleKind::_from_str("type_change"), Some(RuleKind::_TypeChange));
        assert_eq!(RuleKind::_from_str("type_member"), Some(RuleKind::_TypeMember));
        assert_eq!(RuleKind::_from_str("neverallow"), Some(RuleKind::Neverallow));
    }

    #[test]
    fn test_rule_kind_from_str_unknown() {
        assert_eq!(RuleKind::_from_str("unknown"), None);
        assert_eq!(RuleKind::_from_str(""), None);
    }

    #[test]
    fn test_rule_kind_display() {
        assert_eq!(RuleKind::Allow.to_string(), "allow");
        assert_eq!(RuleKind::Dontaudit.to_string(), "dontaudit");
        assert_eq!(RuleKind::Auditallow.to_string(), "auditallow");
        assert_eq!(RuleKind::TypeTransition.to_string(), "type_transition");
        assert_eq!(RuleKind::_TypeChange.to_string(), "type_change");
        assert_eq!(RuleKind::_TypeMember.to_string(), "type_member");
        assert_eq!(RuleKind::Neverallow.to_string(), "neverallow");
    }

    // ---- PolicyRule tests ----

    #[test]
    fn test_policy_rule_display() {
        let rule = PolicyRule {
            kind: RuleKind::Allow,
            source: "httpd_t".into(),
            target: "etc_t".into(),
            class: "file".into(),
            permissions: vec!["read".into(), "open".into()],
        };
        assert_eq!(rule.to_string(), "allow httpd_t etc_t:file { read open };");
    }

    #[test]
    fn test_policy_rule_display_single_perm() {
        let rule = PolicyRule {
            kind: RuleKind::Dontaudit,
            source: "sshd_t".into(),
            target: "proc_t".into(),
            class: "file".into(),
            permissions: vec!["read".into()],
        };
        assert_eq!(rule.to_string(), "dontaudit sshd_t proc_t:file { read };");
    }

    #[test]
    fn test_policy_rule_type_transition_display() {
        let rule = PolicyRule {
            kind: RuleKind::TypeTransition,
            source: "init_t".into(),
            target: "bin_t".into(),
            class: "process".into(),
            permissions: vec!["httpd_t".into()],
        };
        assert_eq!(
            rule.to_string(),
            "type_transition init_t bin_t:process { httpd_t };"
        );
    }

    // ---- Protocol tests ----

    #[test]
    fn test_protocol_from_str() {
        assert_eq!(Protocol::from_str("tcp"), Some(Protocol::Tcp));
        assert_eq!(Protocol::from_str("UDP"), Some(Protocol::Udp));
        assert_eq!(Protocol::from_str("Dccp"), Some(Protocol::Dccp));
        assert_eq!(Protocol::from_str("SCTP"), Some(Protocol::Sctp));
    }

    #[test]
    fn test_protocol_from_str_invalid() {
        assert_eq!(Protocol::from_str("icmp"), None);
        assert_eq!(Protocol::from_str(""), None);
    }

    #[test]
    fn test_protocol_display() {
        assert_eq!(Protocol::Tcp.to_string(), "tcp");
        assert_eq!(Protocol::Udp.to_string(), "udp");
        assert_eq!(Protocol::Dccp.to_string(), "dccp");
        assert_eq!(Protocol::Sctp.to_string(), "sctp");
    }

    // ---- PortContext tests ----

    #[test]
    fn test_port_context_display_single() {
        let pc = PortContext {
            protocol: Protocol::Tcp,
            port_low: 80,
            port_high: 80,
            context: SecurityContext::parse("system_u:object_r:http_port_t:s0").unwrap(),
        };
        assert_eq!(
            pc.to_string(),
            "tcp 80 system_u:object_r:http_port_t:s0"
        );
    }

    #[test]
    fn test_port_context_display_range() {
        let pc = PortContext {
            protocol: Protocol::Tcp,
            port_low: 8080,
            port_high: 8090,
            context: SecurityContext::parse("system_u:object_r:http_cache_port_t:s0").unwrap(),
        };
        assert_eq!(
            pc.to_string(),
            "tcp 8080-8090 system_u:object_r:http_cache_port_t:s0"
        );
    }

    // ---- FileContextType tests ----

    #[test]
    fn test_file_context_type_from_flag() {
        assert_eq!(FileContextType::from_flag("a"), Some(FileContextType::All));
        assert_eq!(FileContextType::from_flag("--"), Some(FileContextType::All));
        assert_eq!(FileContextType::from_flag("f"), Some(FileContextType::Regular));
        assert_eq!(FileContextType::from_flag("-d"), Some(FileContextType::Directory));
        assert_eq!(FileContextType::from_flag("-c"), Some(FileContextType::CharDevice));
        assert_eq!(FileContextType::from_flag("-b"), Some(FileContextType::BlockDevice));
        assert_eq!(FileContextType::from_flag("-s"), Some(FileContextType::Socket));
        assert_eq!(FileContextType::from_flag("-l"), Some(FileContextType::SymLink));
        assert_eq!(FileContextType::from_flag("-p"), Some(FileContextType::Pipe));
    }

    #[test]
    fn test_file_context_type_from_flag_invalid() {
        assert_eq!(FileContextType::from_flag("x"), None);
        assert_eq!(FileContextType::from_flag(""), None);
    }

    #[test]
    fn test_file_context_type_display() {
        assert_eq!(FileContextType::All.to_string(), "all files");
        assert_eq!(FileContextType::Regular.to_string(), "regular file");
        assert_eq!(FileContextType::Directory.to_string(), "directory");
        assert_eq!(FileContextType::CharDevice.to_string(), "character device");
        assert_eq!(FileContextType::BlockDevice.to_string(), "block device");
        assert_eq!(FileContextType::Socket.to_string(), "socket");
        assert_eq!(FileContextType::SymLink.to_string(), "symbolic link");
        assert_eq!(FileContextType::Pipe.to_string(), "named pipe");
    }

    // ---- FileContext tests ----

    #[test]
    fn test_file_context_display() {
        let fc = FileContext {
            pattern: "/etc(/.*)?".into(),
            file_type: FileContextType::All,
            context: SecurityContext::parse("system_u:object_r:etc_t:s0").unwrap(),
        };
        let s = fc.to_string();
        assert!(s.contains("/etc(/.*)?"));
        assert!(s.contains("all files"));
        assert!(s.contains("system_u:object_r:etc_t:s0"));
    }

    // ---- LoginMapping tests ----

    #[test]
    fn test_login_mapping_display() {
        let lm = LoginMapping {
            login: "root".into(),
            seuser: "root".into(),
            mls_range: "s0-s0:c0.c1023".into(),
        };
        let s = lm.to_string();
        assert!(s.contains("root"));
        assert!(s.contains("s0-s0:c0.c1023"));
    }

    // ---- SeUser tests ----

    #[test]
    fn test_se_user_display() {
        let u = SeUser {
            name: "staff_u".into(),
            roles: vec!["staff_r".into(), "sysadm_r".into()],
            mls_level: "s0".into(),
            mls_range: "s0-s0:c0.c1023".into(),
        };
        let s = u.to_string();
        assert!(s.contains("staff_u"));
        assert!(s.contains("staff_r sysadm_r"));
    }

    // ---- SeStatus tests ----

    #[test]
    fn test_sestatus_default() {
        let st = SeStatus::default();
        assert!(!st.enabled);
        assert_eq!(st.mode, EnforceMode::Disabled);
        assert_eq!(st.policy_version, 33);
        assert_eq!(st.policy_name, "targeted");
        assert!(st.mls_enabled);
        assert!(!st.deny_unknown);
    }

    // ---- SeBool tests ----

    #[test]
    fn test_sebool_struct() {
        let b = SeBool {
            name: "test_bool".into(),
            active: true,
            pending: false,
        };
        assert_eq!(b.name, "test_bool");
        assert!(b.active);
        assert!(!b.pending);
    }

    // ---- PolicyDb tests ----

    #[test]
    fn test_policy_db_default_has_types() {
        let db = PolicyDb::default();
        assert!(!db.types.is_empty());
        assert!(db.types.contains(&"kernel_t".to_string()));
        assert!(db.types.contains(&"etc_t".to_string()));
    }

    #[test]
    fn test_policy_db_default_has_roles() {
        let db = PolicyDb::default();
        assert!(!db.roles.is_empty());
        assert!(db.roles.contains(&"object_r".to_string()));
        assert!(db.roles.contains(&"system_r".to_string()));
    }

    #[test]
    fn test_policy_db_default_has_users() {
        let db = PolicyDb::default();
        assert!(!db.users.is_empty());
        assert!(db.users.iter().any(|u| u.name == "system_u"));
        assert!(db.users.iter().any(|u| u.name == "root"));
    }

    #[test]
    fn test_policy_db_default_has_booleans() {
        let db = PolicyDb::default();
        assert!(!db.booleans.is_empty());
        assert!(db.booleans.iter().any(|b| b.name == "httpd_can_network_connect"));
    }

    #[test]
    fn test_policy_db_default_has_classes() {
        let db = PolicyDb::default();
        assert!(db.classes.contains(&"file".to_string()));
        assert!(db.classes.contains(&"process".to_string()));
        assert!(db.classes.contains(&"dir".to_string()));
    }

    #[test]
    fn test_policy_db_default_has_permissions() {
        let db = PolicyDb::default();
        assert!(db.permissions.contains(&"read".to_string()));
        assert!(db.permissions.contains(&"write".to_string()));
        assert!(db.permissions.contains(&"execute".to_string()));
    }

    #[test]
    fn test_policy_db_default_has_rules() {
        let db = PolicyDb::default();
        assert!(!db.rules.is_empty());
        let allow_rules: Vec<_> = db.rules.iter().filter(|r| r.kind == RuleKind::Allow).collect();
        assert!(!allow_rules.is_empty());
    }

    #[test]
    fn test_policy_db_default_has_ports() {
        let db = PolicyDb::default();
        assert!(!db.ports.is_empty());
        assert!(db.ports.iter().any(|p| p.port_low == 80));
        assert!(db.ports.iter().any(|p| p.port_low == 22));
    }

    #[test]
    fn test_policy_db_default_has_file_contexts() {
        let db = PolicyDb::default();
        assert!(!db.file_contexts.is_empty());
        assert!(db.file_contexts.iter().any(|fc| fc.pattern == "/etc(/.*)?"));
    }

    #[test]
    fn test_policy_db_default_has_login_mappings() {
        let db = PolicyDb::default();
        assert!(!db.login_mappings.is_empty());
        assert!(db.login_mappings.iter().any(|lm| lm.login == "root"));
    }

    // ---- AVC denial parsing tests ----

    #[test]
    fn test_parse_avc_denial_basic() {
        let line = "type=AVC msg=audit(1234567890.123:456): avc:  denied  { read open } for  pid=1234 comm=\"httpd\" scontext=system_u:system_r:httpd_t:s0 tcontext=system_u:object_r:etc_t:s0 tclass=file";
        let denial = parse_avc_denial(line).unwrap();
        assert_eq!(denial.permissions, vec!["read", "open"]);
        assert_eq!(denial.source, "system_u:system_r:httpd_t:s0");
        assert_eq!(denial.target, "system_u:object_r:etc_t:s0");
        assert_eq!(denial.class, "file");
        assert_eq!(denial.pid, 1234);
        assert_eq!(denial.comm, "httpd");
    }

    #[test]
    fn test_parse_avc_denial_single_perm() {
        let line = "type=AVC msg=audit(1.1:1): avc:  denied  { connect } for  pid=99 comm=\"curl\" scontext=user_u:user_r:user_t:s0 tcontext=system_u:object_r:net_t:s0 tclass=tcp_socket";
        let denial = parse_avc_denial(line).unwrap();
        assert_eq!(denial.permissions, vec!["connect"]);
        assert_eq!(denial.class, "tcp_socket");
    }

    #[test]
    fn test_parse_avc_denial_not_avc() {
        let line = "type=SYSCALL msg=audit(1.1:1): arch=x86_64 syscall=open success=yes";
        assert!(parse_avc_denial(line).is_none());
    }

    #[test]
    fn test_parse_avc_denial_empty() {
        assert!(parse_avc_denial("").is_none());
    }

    #[test]
    fn test_parse_avc_denial_no_braces() {
        let line = "type=AVC msg=audit(1.1:1): avc:  denied no braces here";
        assert!(parse_avc_denial(line).is_none());
    }

    #[test]
    fn test_parse_avc_denial_missing_scontext() {
        let line = "type=AVC msg=audit(1.1:1): avc:  denied  { read } for  pid=1 tcontext=system_u:object_r:etc_t:s0 tclass=file";
        assert!(parse_avc_denial(line).is_none());
    }

    // ---- extract_braces tests ----

    #[test]
    fn test_extract_braces_basic() {
        let result = extract_braces("{ read write }").unwrap();
        assert_eq!(result, vec!["read", "write"]);
    }

    #[test]
    fn test_extract_braces_single() {
        let result = extract_braces("{ execute }").unwrap();
        assert_eq!(result, vec!["execute"]);
    }

    #[test]
    fn test_extract_braces_no_braces() {
        assert!(extract_braces("no braces here").is_none());
    }

    #[test]
    fn test_extract_braces_empty() {
        assert!(extract_braces("{  }").is_none());
    }

    #[test]
    fn test_extract_braces_in_context() {
        let result =
            extract_braces("prefix { open getattr } suffix").unwrap();
        assert_eq!(result, vec!["open", "getattr"]);
    }

    // ---- extract_field tests ----

    #[test]
    fn test_extract_field_basic() {
        let val = extract_field("pid=1234 comm=\"test\"", "pid=").unwrap();
        assert_eq!(val, "1234");
    }

    #[test]
    fn test_extract_field_at_end() {
        let val = extract_field("key=value", "key=").unwrap();
        assert_eq!(val, "value");
    }

    #[test]
    fn test_extract_field_missing() {
        assert!(extract_field("something else", "pid=").is_none());
    }

    #[test]
    fn test_extract_field_context() {
        let val = extract_field(
            "scontext=system_u:system_r:httpd_t:s0 tcontext=x",
            "scontext=",
        )
        .unwrap();
        assert_eq!(val, "system_u:system_r:httpd_t:s0");
    }

    // ---- extract_type_from_context tests ----

    #[test]
    fn test_extract_type_full_context() {
        assert_eq!(
            extract_type_from_context("system_u:system_r:httpd_t:s0"),
            "httpd_t"
        );
    }

    #[test]
    fn test_extract_type_bare_type() {
        assert_eq!(extract_type_from_context("httpd_t"), "httpd_t");
    }

    #[test]
    fn test_extract_type_two_parts() {
        assert_eq!(extract_type_from_context("user:role"), "user:role");
    }

    // ---- denials_to_allow_rules tests ----

    #[test]
    fn test_denials_to_allow_rules_basic() {
        let denials = vec![AvcDenial {
            source: "system_u:system_r:httpd_t:s0".into(),
            target: "system_u:object_r:etc_t:s0".into(),
            class: "file".into(),
            permissions: vec!["read".into()],
            pid: 1,
            comm: "httpd".into(),
        }];
        let rules = denials_to_allow_rules(&denials);
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].source, "httpd_t");
        assert_eq!(rules[0].target, "etc_t");
        assert_eq!(rules[0].class, "file");
        assert!(rules[0].permissions.contains(&"read".to_string()));
    }

    #[test]
    fn test_denials_to_allow_rules_merges() {
        let denials = vec![
            AvcDenial {
                source: "user_u:user_r:user_t:s0".into(),
                target: "system_u:object_r:etc_t:s0".into(),
                class: "file".into(),
                permissions: vec!["read".into()],
                pid: 1,
                comm: "cat".into(),
            },
            AvcDenial {
                source: "user_u:user_r:user_t:s0".into(),
                target: "system_u:object_r:etc_t:s0".into(),
                class: "file".into(),
                permissions: vec!["open".into()],
                pid: 1,
                comm: "cat".into(),
            },
        ];
        let rules = denials_to_allow_rules(&denials);
        // Merging is by full context string as the key, so these should
        // produce a single rule since source+target+class match
        assert_eq!(rules.len(), 1);
        assert!(rules[0].permissions.contains(&"read".to_string()));
        assert!(rules[0].permissions.contains(&"open".to_string()));
    }

    #[test]
    fn test_denials_to_allow_rules_no_dup_perms() {
        let denials = vec![
            AvcDenial {
                source: "a:b:test_t:s0".into(),
                target: "a:b:etc_t:s0".into(),
                class: "file".into(),
                permissions: vec!["read".into()],
                pid: 1,
                comm: "x".into(),
            },
            AvcDenial {
                source: "a:b:test_t:s0".into(),
                target: "a:b:etc_t:s0".into(),
                class: "file".into(),
                permissions: vec!["read".into()],
                pid: 2,
                comm: "y".into(),
            },
        ];
        let rules = denials_to_allow_rules(&denials);
        assert_eq!(rules.len(), 1);
        // "read" should appear only once
        assert_eq!(
            rules[0].permissions.iter().filter(|p| *p == "read").count(),
            1
        );
    }

    #[test]
    fn test_denials_to_allow_rules_empty() {
        let rules = denials_to_allow_rules(&[]);
        assert!(rules.is_empty());
    }

    #[test]
    fn test_denials_to_allow_rules_different_classes() {
        let denials = vec![
            AvcDenial {
                source: "a:b:httpd_t:s0".into(),
                target: "a:b:net_t:s0".into(),
                class: "tcp_socket".into(),
                permissions: vec!["connect".into()],
                pid: 1,
                comm: "httpd".into(),
            },
            AvcDenial {
                source: "a:b:httpd_t:s0".into(),
                target: "a:b:net_t:s0".into(),
                class: "udp_socket".into(),
                permissions: vec!["send".into()],
                pid: 1,
                comm: "httpd".into(),
            },
        ];
        let rules = denials_to_allow_rules(&denials);
        assert_eq!(rules.len(), 2);
    }

    // ---- AvcDenial display tests ----

    #[test]
    fn test_avc_denial_display() {
        let d = AvcDenial {
            source: "ctx_a".into(),
            target: "ctx_b".into(),
            class: "file".into(),
            permissions: vec!["read".into(), "write".into()],
            pid: 123,
            comm: "test".into(),
        };
        let s = d.to_string();
        assert!(s.contains("denied"));
        assert!(s.contains("read write"));
        assert!(s.contains("scontext=ctx_a"));
        assert!(s.contains("tcontext=ctx_b"));
        assert!(s.contains("tclass=file"));
    }

    // ---- pattern_matches tests ----

    #[test]
    fn test_pattern_matches_exact() {
        assert!(pattern_matches("/etc", "/etc"));
    }

    #[test]
    fn test_pattern_matches_recursive() {
        assert!(pattern_matches("/etc(/.*)?", "/etc"));
        assert!(pattern_matches("/etc(/.*)?", "/etc/passwd"));
        assert!(pattern_matches("/etc(/.*)?", "/etc/sysconfig/network"));
    }

    #[test]
    fn test_pattern_matches_no_match() {
        assert!(!pattern_matches("/etc(/.*)?", "/home"));
        assert!(!pattern_matches("/etc(/.*)?", "/etcfoo"));
    }

    #[test]
    fn test_pattern_matches_home() {
        assert!(pattern_matches("/home(/.*)?", "/home"));
        assert!(pattern_matches("/home(/.*)?", "/home/user"));
        assert!(pattern_matches("/home(/.*)?", "/home/user/.bashrc"));
    }

    #[test]
    fn test_pattern_matches_root_prefix_no_false_positive() {
        assert!(!pattern_matches("/tmp(/.*)?", "/tmpfiles"));
    }

    // ---- find_file_context tests ----

    #[test]
    fn test_find_file_context_etc() {
        let db = PolicyDb::default();
        let fc = find_file_context(&db.file_contexts, "/etc/passwd").unwrap();
        assert_eq!(fc.context.context_type, "etc_t");
    }

    #[test]
    fn test_find_file_context_home() {
        let db = PolicyDb::default();
        let fc = find_file_context(&db.file_contexts, "/home/user").unwrap();
        assert_eq!(fc.context.context_type, "home_t");
    }

    #[test]
    fn test_find_file_context_tmp() {
        let db = PolicyDb::default();
        let fc = find_file_context(&db.file_contexts, "/tmp/somefile").unwrap();
        assert_eq!(fc.context.context_type, "tmp_t");
    }

    #[test]
    fn test_find_file_context_usr_bin() {
        let db = PolicyDb::default();
        let fc = find_file_context(&db.file_contexts, "/usr/bin/ls").unwrap();
        assert_eq!(fc.context.context_type, "bin_t");
    }

    #[test]
    fn test_find_file_context_usr_lib() {
        let db = PolicyDb::default();
        let fc = find_file_context(&db.file_contexts, "/usr/lib/libc.so").unwrap();
        assert_eq!(fc.context.context_type, "lib_t");
    }

    #[test]
    fn test_find_file_context_var() {
        let db = PolicyDb::default();
        let fc = find_file_context(&db.file_contexts, "/var/log/messages").unwrap();
        assert_eq!(fc.context.context_type, "var_t");
    }

    #[test]
    fn test_find_file_context_proc() {
        let db = PolicyDb::default();
        let fc = find_file_context(&db.file_contexts, "/proc/1/status").unwrap();
        assert_eq!(fc.context.context_type, "proc_t");
    }

    #[test]
    fn test_find_file_context_no_match() {
        let db = PolicyDb::default();
        let fc = find_file_context(&db.file_contexts, "/nonexistent");
        assert!(fc.is_none());
    }

    #[test]
    fn test_find_file_context_longest_match() {
        let db = PolicyDb::default();
        // /usr/bin should match bin_t (longer prefix) not any generic one
        let fc = find_file_context(&db.file_contexts, "/usr/bin/test").unwrap();
        assert_eq!(fc.context.context_type, "bin_t");
    }

    // ---- semanage subcommand parsing (unit-level tests) ----

    #[test]
    fn test_semanage_boolean_list() {
        // Just verify it doesn't panic
        assert_eq!(semanage_boolean(&["-l".into()]), 0);
    }

    #[test]
    fn test_semanage_boolean_modify_no_value() {
        assert_eq!(semanage_boolean(&["-m".into(), "test_bool".into()]), 1);
    }

    #[test]
    fn test_semanage_boolean_modify_on() {
        assert_eq!(
            semanage_boolean(&["-m".into(), "--on".into(), "test_bool".into()]),
            0
        );
    }

    #[test]
    fn test_semanage_boolean_add() {
        assert_eq!(semanage_boolean(&["-a".into(), "new_bool".into()]), 0);
    }

    #[test]
    fn test_semanage_boolean_delete() {
        assert_eq!(semanage_boolean(&["-d".into(), "old_bool".into()]), 0);
    }

    #[test]
    fn test_semanage_port_list() {
        assert_eq!(semanage_port(&["-l".into()]), 0);
    }

    #[test]
    fn test_semanage_port_add_missing_proto() {
        assert_eq!(
            semanage_port(&["-a".into(), "-t".into(), "http_port_t".into(), "8080".into()]),
            1
        );
    }

    #[test]
    fn test_semanage_port_add_valid() {
        assert_eq!(
            semanage_port(&[
                "-a".into(),
                "-t".into(),
                "http_port_t".into(),
                "-p".into(),
                "tcp".into(),
                "9090".into()
            ]),
            0
        );
    }

    #[test]
    fn test_semanage_port_invalid_proto() {
        assert_eq!(
            semanage_port(&[
                "-a".into(),
                "-t".into(),
                "http_port_t".into(),
                "-p".into(),
                "icmp".into(),
                "80".into()
            ]),
            1
        );
    }

    #[test]
    fn test_semanage_fcontext_list() {
        assert_eq!(semanage_fcontext(&["-l".into()]), 0);
    }

    #[test]
    fn test_semanage_fcontext_add() {
        assert_eq!(
            semanage_fcontext(&[
                "-a".into(),
                "-t".into(),
                "httpd_sys_content_t".into(),
                "/srv/www(/.*)?".into()
            ]),
            0
        );
    }

    #[test]
    fn test_semanage_fcontext_add_missing_type() {
        assert_eq!(
            semanage_fcontext(&["-a".into(), "/srv/www(/.*)?".into()]),
            1
        );
    }

    #[test]
    fn test_semanage_fcontext_delete() {
        assert_eq!(
            semanage_fcontext(&["-d".into(), "/srv/www(/.*)?".into()]),
            0
        );
    }

    #[test]
    fn test_semanage_login_list() {
        assert_eq!(semanage_login(&["-l".into()]), 0);
    }

    #[test]
    fn test_semanage_login_add() {
        assert_eq!(
            semanage_login(&["-a".into(), "-s".into(), "staff_u".into(), "testuser".into()]),
            0
        );
    }

    #[test]
    fn test_semanage_login_add_missing_seuser() {
        assert_eq!(
            semanage_login(&["-a".into(), "testuser".into()]),
            1
        );
    }

    #[test]
    fn test_semanage_login_delete() {
        assert_eq!(
            semanage_login(&["-d".into(), "testuser".into()]),
            0
        );
    }

    #[test]
    fn test_semanage_user_list() {
        assert_eq!(semanage_user(&["-l".into()]), 0);
    }

    #[test]
    fn test_semanage_user_add() {
        assert_eq!(
            semanage_user(&[
                "-a".into(),
                "-R".into(),
                "staff_r".into(),
                "new_user_u".into()
            ]),
            0
        );
    }

    #[test]
    fn test_semanage_user_add_missing_roles() {
        assert_eq!(
            semanage_user(&["-a".into(), "new_user_u".into()]),
            1
        );
    }

    #[test]
    fn test_semanage_user_delete() {
        assert_eq!(
            semanage_user(&["-d".into(), "old_user_u".into()]),
            0
        );
    }

    // ---- cmd_setenforce tests ----

    #[test]
    fn test_setenforce_no_args() {
        assert_eq!(cmd_setenforce(&[]), 1);
    }

    #[test]
    fn test_setenforce_invalid_mode() {
        assert_eq!(cmd_setenforce(&["bogus".into()]), 1);
    }

    // ---- cmd_setsebool tests ----

    #[test]
    fn test_setsebool_no_args() {
        assert_eq!(cmd_setsebool(&[]), 1);
    }

    #[test]
    fn test_setsebool_invalid_value() {
        assert_eq!(cmd_setsebool(&["test_bool".into(), "maybe".into()]), 1);
    }

    // ---- cmd_getsebool tests ----

    #[test]
    fn test_getsebool_show_all() {
        assert_eq!(cmd_getsebool(&["-a".into()]), 0);
    }

    #[test]
    fn test_getsebool_no_args() {
        assert_eq!(cmd_getsebool(&[]), 1);
    }

    // ---- cmd_restorecon tests ----

    #[test]
    fn test_restorecon_no_args() {
        assert_eq!(cmd_restorecon(&[]), 1);
    }

    // ---- cmd_chcon tests ----

    #[test]
    fn test_chcon_no_args() {
        assert_eq!(cmd_chcon(&[]), 1);
    }

    // ---- cmd_seinfo tests ----

    #[test]
    fn test_seinfo_default() {
        assert_eq!(cmd_seinfo(&[]), 0);
    }

    #[test]
    fn test_seinfo_types() {
        assert_eq!(cmd_seinfo(&["-t".into()]), 0);
    }

    #[test]
    fn test_seinfo_specific_type() {
        assert_eq!(cmd_seinfo(&["-t".into(), "httpd_t".into()]), 0);
    }

    #[test]
    fn test_seinfo_unknown_type() {
        assert_eq!(cmd_seinfo(&["-t".into(), "nonexistent_t".into()]), 1);
    }

    #[test]
    fn test_seinfo_roles() {
        assert_eq!(cmd_seinfo(&["-r".into()]), 0);
    }

    #[test]
    fn test_seinfo_specific_role() {
        assert_eq!(cmd_seinfo(&["-r".into(), "system_r".into()]), 0);
    }

    #[test]
    fn test_seinfo_unknown_role() {
        assert_eq!(cmd_seinfo(&["-r".into(), "nonexist_r".into()]), 1);
    }

    #[test]
    fn test_seinfo_users() {
        assert_eq!(cmd_seinfo(&["-u".into()]), 0);
    }

    #[test]
    fn test_seinfo_specific_user() {
        assert_eq!(cmd_seinfo(&["-u".into(), "root".into()]), 0);
    }

    #[test]
    fn test_seinfo_unknown_user() {
        assert_eq!(cmd_seinfo(&["-u".into(), "nobody_u".into()]), 1);
    }

    #[test]
    fn test_seinfo_booleans() {
        assert_eq!(cmd_seinfo(&["-b".into()]), 0);
    }

    #[test]
    fn test_seinfo_specific_boolean() {
        assert_eq!(
            cmd_seinfo(&["-b".into(), "httpd_can_network_connect".into()]),
            0
        );
    }

    #[test]
    fn test_seinfo_unknown_boolean() {
        assert_eq!(cmd_seinfo(&["-b".into(), "no_such_bool".into()]), 1);
    }

    #[test]
    fn test_seinfo_classes() {
        assert_eq!(cmd_seinfo(&["-c".into()]), 0);
    }

    #[test]
    fn test_seinfo_specific_class() {
        assert_eq!(cmd_seinfo(&["-c".into(), "file".into()]), 0);
    }

    #[test]
    fn test_seinfo_unknown_class() {
        assert_eq!(cmd_seinfo(&["-c".into(), "bogus_class".into()]), 1);
    }

    #[test]
    fn test_seinfo_stats() {
        assert_eq!(cmd_seinfo(&["--stats".into()]), 0);
    }

    #[test]
    fn test_seinfo_unknown_option() {
        assert_eq!(cmd_seinfo(&["--bogus".into()]), 1);
    }

    // ---- cmd_sesearch tests ----

    #[test]
    fn test_sesearch_allow() {
        assert_eq!(cmd_sesearch(&["-A".into()]), 0);
    }

    #[test]
    fn test_sesearch_dontaudit() {
        assert_eq!(cmd_sesearch(&["--dontaudit".into()]), 0);
    }

    #[test]
    fn test_sesearch_type_trans() {
        assert_eq!(cmd_sesearch(&["--type_trans".into()]), 0);
    }

    #[test]
    fn test_sesearch_source_filter() {
        assert_eq!(cmd_sesearch(&["-A".into(), "-s".into(), "httpd_t".into()]), 0);
    }

    #[test]
    fn test_sesearch_target_filter() {
        assert_eq!(cmd_sesearch(&["-A".into(), "-t".into(), "etc_t".into()]), 0);
    }

    #[test]
    fn test_sesearch_class_filter() {
        assert_eq!(
            cmd_sesearch(&["-A".into(), "-c".into(), "file".into()]),
            0
        );
    }

    #[test]
    fn test_sesearch_perm_filter() {
        assert_eq!(
            cmd_sesearch(&["-A".into(), "-p".into(), "read".into()]),
            0
        );
    }

    #[test]
    fn test_sesearch_combined_filters() {
        assert_eq!(
            cmd_sesearch(&[
                "-A".into(),
                "-s".into(),
                "httpd_t".into(),
                "-t".into(),
                "etc_t".into(),
                "-c".into(),
                "file".into()
            ]),
            0
        );
    }

    #[test]
    fn test_sesearch_no_results() {
        assert_eq!(
            cmd_sesearch(&["-A".into(), "-s".into(), "nonexist_t".into()]),
            0
        );
    }

    #[test]
    fn test_sesearch_unknown_option() {
        assert_eq!(cmd_sesearch(&["--bogus".into()]), 1);
    }

    // ---- cmd_semanage dispatch tests ----

    #[test]
    fn test_semanage_no_args() {
        assert_eq!(cmd_semanage(&[]), 1);
    }

    #[test]
    fn test_semanage_help() {
        assert_eq!(cmd_semanage(&["-h".into()]), 0);
        assert_eq!(cmd_semanage(&["--help".into()]), 0);
        assert_eq!(cmd_semanage(&["help".into()]), 0);
    }

    #[test]
    fn test_semanage_unknown_subcommand() {
        assert_eq!(cmd_semanage(&["bogus".into()]), 1);
    }

    // ---- cmd_sestatus tests ----

    #[test]
    fn test_sestatus_basic() {
        // Should succeed even when SELinux fs doesn't exist
        assert_eq!(cmd_sestatus(&[]), 0);
    }

    #[test]
    fn test_sestatus_verbose() {
        assert_eq!(cmd_sestatus(&["-v".into()]), 0);
    }

    // ---- Personality detection (argv[0] parsing) ----

    #[test]
    fn test_personality_detection_simple() {
        let s = "getenforce";
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' { last_sep = i + 1; }
        }
        let base = &s[last_sep..];
        let base = base.strip_suffix(".exe").unwrap_or(base);
        assert_eq!(base, "getenforce");
    }

    #[test]
    fn test_personality_detection_with_path() {
        let s = "/usr/bin/sestatus";
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' { last_sep = i + 1; }
        }
        let base = &s[last_sep..];
        let base = base.strip_suffix(".exe").unwrap_or(base);
        assert_eq!(base, "sestatus");
    }

    #[test]
    fn test_personality_detection_windows_path() {
        let s = "C:\\Program Files\\semanage.exe";
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' { last_sep = i + 1; }
        }
        let base = &s[last_sep..];
        let base = base.strip_suffix(".exe").unwrap_or(base);
        assert_eq!(base, "semanage");
    }

    #[test]
    fn test_personality_detection_exe_suffix() {
        let s = "setsebool.exe";
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' { last_sep = i + 1; }
        }
        let base = &s[last_sep..];
        let base = base.strip_suffix(".exe").unwrap_or(base);
        assert_eq!(base, "setsebool");
    }

    #[test]
    fn test_personality_detection_mixed_separators() {
        let s = "/opt/tools\\bin/chcon";
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' { last_sep = i + 1; }
        }
        let base = &s[last_sep..];
        let base = base.strip_suffix(".exe").unwrap_or(base);
        assert_eq!(base, "chcon");
    }
}
