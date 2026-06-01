//! OurOS Privileged Command Execution Utility
//!
//! Multi-personality binary providing `sudo`, `sudoedit`/`visudo`, and
//! `sudoreplay` functionality. Personality is detected via `argv[0]` basename,
//! stripping any path prefix and `.exe` suffix.
//!
//! # Personalities
//!
//! - **sudo** (default) — execute a command as another user
//! - **sudoedit** — safely edit files with elevated privileges
//! - **visudo** — edit the sudoers file with syntax checking
//! - **sudoreplay** — replay recorded sudo session logs
//!
//! # sudo Usage
//!
//! ```text
//! sudo [-u user] [-g group] [-i] [-s] [-b] [-n] [-E] [-p prompt] [--] command [args...]
//! sudo -l               List user's privileges
//! sudo -v               Validate / extend timestamp
//! sudo -k               Invalidate timestamp
//! sudo -K               Remove timestamp entirely
//! sudo -e file...       Edit files (sudoedit mode)
//! ```
//!
//! # visudo Usage
//!
//! ```text
//! visudo                 Edit /etc/sudoers
//! visudo -c              Check syntax only
//! visudo -f file         Edit alternate sudoers file
//! visudo -s              Strict mode (error on warnings)
//! ```
//!
//! # sudoreplay Usage
//!
//! ```text
//! sudoreplay -l          List recorded sessions
//! sudoreplay -d dir      Replay from specific directory
//! sudoreplay -s factor   Set speed factor for replay
//! sudoreplay [session]   Replay a specific session
//! ```

#![deny(clippy::all)]

use std::collections::HashMap;
use std::env;
use std::fmt;
use std::fs;
use std::io::{self, Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::process;
use std::time::{SystemTime, UNIX_EPOCH};

// ============================================================================
// Constants
// ============================================================================

const SUDOERS_PATH: &str = "/etc/sudoers";
const TIMESTAMP_DIR: &str = "/var/run/sudo/ts";
const SUDO_LOG_PATH: &str = "/var/log/sudo.log";
const SUDO_IO_DIR: &str = "/var/log/sudo-io";
const DEFAULT_TIMEOUT: u64 = 900; // 15 minutes in seconds
const DEFAULT_EDITOR: &str = "/usr/bin/vi";
const DEFAULT_PROMPT: &str = "[sudo] password for %u: ";

/// Environment variables preserved by default when env_reset is active.
const DEFAULT_ENV_KEEP: &[&str] = &[
    "TERM", "PATH", "HOME", "SHELL", "LOGNAME", "USER", "DISPLAY",
    "XAUTHORITY", "LANG", "LC_ALL", "LC_COLLATE", "LC_CTYPE",
    "LC_MESSAGES", "LC_MONETARY", "LC_NUMERIC", "LC_TIME", "TZ",
];

/// Environment variables that are always removed for security.
const ENV_BLACKLIST: &[&str] = &[
    "LD_PRELOAD", "LD_LIBRARY_PATH", "LD_AUDIT", "LD_BIND_NOW",
    "LD_DEBUG", "LD_DYNAMIC_WEAK", "LD_ORIGIN_PATH", "LD_PROFILE",
    "LD_SHOW_AUXV", "LD_USE_LOAD_BIAS", "LOCALDOMAIN", "RES_OPTIONS",
    "HOSTALIASES", "NLSPATH", "PATH_LOCALE", "TERMINFO", "TERMINFO_DIRS",
    "TERMPATH",
];

// ============================================================================
// Personality detection
// ============================================================================

/// The personality under which the binary was invoked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Personality {
    Sudo,
    Sudoedit,
    Visudo,
    Sudoreplay,
}

impl fmt::Display for Personality {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sudo => write!(f, "sudo"),
            Self::Sudoedit => write!(f, "sudoedit"),
            Self::Visudo => write!(f, "visudo"),
            Self::Sudoreplay => write!(f, "sudoreplay"),
        }
    }
}

fn detect_personality(argv0: &str) -> Personality {
    let bytes = argv0.as_bytes();
    let mut last_sep = 0;
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'/' || b == b'\\' {
            last_sep = i + 1;
        }
    }
    let base = &argv0[last_sep..];
    let base = base.strip_suffix(".exe").unwrap_or(base);

    match base {
        "sudoedit" => Personality::Sudoedit,
        "visudo" => Personality::Visudo,
        "sudoreplay" => Personality::Sudoreplay,
        _ => Personality::Sudo,
    }
}

// ============================================================================
// Error types
// ============================================================================

/// Unified error type for sudo operations.
#[derive(Debug)]
enum SudoError {
    _PermissionDenied(String),
    ParseError(String),
    IoError(String),
    InvalidConfig(String),
    AuthError(String),
    UsageError(String),
    TimestampError(String),
    LockError(String),
}

impl fmt::Display for SudoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::_PermissionDenied(msg) => write!(f, "permission denied: {msg}"),
            Self::ParseError(msg) => write!(f, "parse error: {msg}"),
            Self::IoError(msg) => write!(f, "I/O error: {msg}"),
            Self::InvalidConfig(msg) => write!(f, "invalid configuration: {msg}"),
            Self::AuthError(msg) => write!(f, "authentication error: {msg}"),
            Self::UsageError(msg) => write!(f, "usage error: {msg}"),
            Self::TimestampError(msg) => write!(f, "timestamp error: {msg}"),
            Self::LockError(msg) => write!(f, "lock error: {msg}"),
        }
    }
}

impl From<io::Error> for SudoError {
    fn from(e: io::Error) -> Self {
        Self::IoError(e.to_string())
    }
}

// ============================================================================
// Sudoers data model
// ============================================================================

/// A parsed alias (User_Alias, Host_Alias, Cmnd_Alias, Runas_Alias).
#[derive(Debug, Clone)]
struct _Alias {
    _name: String,
    _members: Vec<String>,
}

/// A Defaults directive from the sudoers file.
#[derive(Debug, Clone)]
struct DefaultsDirective {
    /// The scope (empty = global, "user:" prefix, "host:" prefix, etc.)
    scope: String,
    /// Key-value settings.
    settings: Vec<(String, String)>,
}

/// Represents who a command may be run as.
#[derive(Debug, Clone)]
struct RunasSpec {
    users: Vec<String>,
    groups: Vec<String>,
}

impl Default for RunasSpec {
    fn default() -> Self {
        Self {
            users: vec!["root".to_string()],
            groups: Vec::new(),
        }
    }
}

/// A single command specification in a privilege entry.
#[derive(Debug, Clone)]
struct CmndSpec {
    /// Whether NOPASSWD is set for this command.
    nopasswd: bool,
    /// Whether NOEXEC is set for this command.
    noexec: bool,
    /// Whether SETENV is allowed.
    setenv: bool,
    /// The command pattern (path or ALL).
    command: String,
    /// Optional arguments pattern (empty = any args).
    args: String,
}

/// A complete privilege specification line.
#[derive(Debug, Clone)]
struct PrivilegeSpec {
    /// The user or group this applies to (may be an alias name, %group, etc.)
    users: Vec<String>,
    /// Hosts this applies on.
    hosts: Vec<String>,
    /// Runas specification.
    runas: RunasSpec,
    /// Allowed commands.
    commands: Vec<CmndSpec>,
}

/// Complete parsed sudoers configuration.
#[derive(Debug, Clone)]
struct SudoersConfig {
    user_aliases: HashMap<String, Vec<String>>,
    host_aliases: HashMap<String, Vec<String>>,
    cmnd_aliases: HashMap<String, Vec<String>>,
    runas_aliases: HashMap<String, Vec<String>>,
    defaults: Vec<DefaultsDirective>,
    privileges: Vec<PrivilegeSpec>,
}

impl SudoersConfig {
    fn new() -> Self {
        Self {
            user_aliases: HashMap::new(),
            host_aliases: HashMap::new(),
            cmnd_aliases: HashMap::new(),
            runas_aliases: HashMap::new(),
            defaults: Vec::new(),
            privileges: Vec::new(),
        }
    }

    /// Get the value of a Defaults setting (global scope).
    fn get_default(&self, key: &str) -> Option<&str> {
        for d in &self.defaults {
            if d.scope.is_empty() {
                for (k, v) in &d.settings {
                    if k == key {
                        return Some(v.as_str());
                    }
                }
            }
        }
        None
    }

    /// Check if a Defaults flag is set (boolean setting).
    fn is_default_set(&self, key: &str) -> bool {
        self.get_default(key).is_some_and(|v| v != "false" && v != "0")
    }

    /// Get env_keep list from Defaults.
    fn env_keep_list(&self) -> Vec<String> {
        let mut result: Vec<String> = DEFAULT_ENV_KEEP.iter().map(|s| (*s).to_string()).collect();
        for d in &self.defaults {
            if d.scope.is_empty() {
                for (k, v) in &d.settings {
                    // Handle env_keep, env_keep+=, and env_keep+ (the `+` remains
                    // when `+=` is split at the first `=`).
                    if k == "env_keep" || k == "env_keep+=" || k == "env_keep+" {
                        for var in v.split_whitespace() {
                            let var = var.trim_matches('"');
                            if !result.iter().any(|r| r == var) {
                                result.push(var.to_string());
                            }
                        }
                    }
                }
            }
        }
        result
    }

    /// Get env_check list from Defaults.
    fn env_check_list(&self) -> Vec<String> {
        let mut result = Vec::new();
        for d in &self.defaults {
            if d.scope.is_empty() {
                for (k, v) in &d.settings {
                    if k == "env_check" || k == "env_check+=" || k == "env_check+" {
                        for var in v.split_whitespace() {
                            let var = var.trim_matches('"');
                            if !result.iter().any(|r: &String| r == var) {
                                result.push(var.to_string());
                            }
                        }
                    }
                }
            }
        }
        result
    }

    /// Get the timestamp_timeout (in seconds).
    fn timestamp_timeout(&self) -> u64 {
        self.get_default("timestamp_timeout")
            .and_then(|v| v.parse::<f64>().ok())
            .map(|minutes| {
                if minutes < 0.0 {
                    // Negative means never expire
                    u64::MAX
                } else {
                    (minutes * 60.0) as u64
                }
            })
            .unwrap_or(DEFAULT_TIMEOUT)
    }
}

// ============================================================================
// Sudoers parser
// ============================================================================

/// Parse the sudoers file content into a `SudoersConfig`.
fn parse_sudoers(content: &str) -> Result<SudoersConfig, SudoError> {
    let mut config = SudoersConfig::new();
    let mut continued_line = String::new();

    for raw_line in content.lines() {
        let trimmed = raw_line.trim();

        // Skip comments and empty lines.
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        // Handle line continuation (trailing backslash).
        if let Some(stripped) = trimmed.strip_suffix('\\') {
            continued_line.push_str(stripped);
            continued_line.push(' ');
            continue;
        }

        let line = if continued_line.is_empty() {
            trimmed.to_string()
        } else {
            continued_line.push_str(trimmed);
            let result = continued_line.clone();
            continued_line.clear();
            result
        };

        parse_sudoers_line(&line, &mut config)?;
    }

    // Handle any remaining continued line.
    if !continued_line.is_empty() {
        parse_sudoers_line(continued_line.trim(), &mut config)?;
    }

    Ok(config)
}

/// Parse a single (possibly joined) sudoers line.
fn parse_sudoers_line(line: &str, config: &mut SudoersConfig) -> Result<(), SudoError> {
    // Alias definitions.
    if let Some(rest) = line.strip_prefix("User_Alias") {
        parse_alias(rest.trim(), &mut config.user_aliases)?;
        return Ok(());
    }
    if let Some(rest) = line.strip_prefix("Host_Alias") {
        parse_alias(rest.trim(), &mut config.host_aliases)?;
        return Ok(());
    }
    if let Some(rest) = line.strip_prefix("Cmnd_Alias") {
        parse_alias(rest.trim(), &mut config.cmnd_aliases)?;
        return Ok(());
    }
    if let Some(rest) = line.strip_prefix("Runas_Alias") {
        parse_alias(rest.trim(), &mut config.runas_aliases)?;
        return Ok(());
    }

    // Defaults directive.
    if let Some(rest) = line.strip_prefix("Defaults") {
        parse_defaults(rest, config)?;
        return Ok(());
    }

    // #include / #includedir (legacy format — also @include / @includedir).
    if line.starts_with("#include")
        || line.starts_with("@include")
        || line.starts_with("#includedir")
        || line.starts_with("@includedir")
    {
        // In OurOS, includes are handled at a higher level; skip in parsing.
        return Ok(());
    }

    // Otherwise it is a user privilege specification.
    parse_privilege_spec(line, config)?;
    Ok(())
}

/// Parse an alias definition: `NAME = member1, member2, ...`
fn parse_alias(
    text: &str,
    aliases: &mut HashMap<String, Vec<String>>,
) -> Result<(), SudoError> {
    // Multiple aliases can be on one line, separated by `:`.
    for alias_part in text.split(':') {
        let alias_part = alias_part.trim();
        let eq_pos = alias_part
            .find('=')
            .ok_or_else(|| SudoError::ParseError(format!("missing '=' in alias: {alias_part}")))?;
        let name = alias_part[..eq_pos].trim().to_string();
        let members_str = &alias_part[eq_pos + 1..];
        let members: Vec<String> = members_str
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        if name.is_empty() {
            return Err(SudoError::ParseError("empty alias name".to_string()));
        }
        if !name.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
            return Err(SudoError::ParseError(format!(
                "alias name must start with uppercase: {name}"
            )));
        }
        aliases.insert(name, members);
    }
    Ok(())
}

/// Parse a Defaults directive.
fn parse_defaults(rest: &str, config: &mut SudoersConfig) -> Result<(), SudoError> {
    let rest = rest.trim();

    // Determine scope: Defaults, Defaults:user, Defaults@host, Defaults!cmnd,
    // Defaults>runas.
    let (scope, settings_str) = if rest.starts_with(':')
        || rest.starts_with('@')
        || rest.starts_with('!')
        || rest.starts_with('>')
    {
        // Scoped defaults.
        let scope_char = &rest[..1];
        let after = &rest[1..];
        if let Some(space_pos) = after.find(|c: char| c.is_whitespace()) {
            let scope_name = after[..space_pos].trim().to_string();
            let settings = after[space_pos..].trim();
            (format!("{scope_char}{scope_name}"), settings)
        } else {
            // Just a scope with no settings — treat the whole thing as a flag.
            return Ok(());
        }
    } else {
        // Global defaults.
        (String::new(), rest)
    };

    let mut settings = Vec::new();
    for part in settings_str.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }

        if let Some(eq_pos) = part.find('=') {
            let key = part[..eq_pos].trim().to_string();
            let val = part[eq_pos + 1..].trim().trim_matches('"').to_string();
            settings.push((key, val));
        } else if let Some(stripped) = part.strip_prefix('!') {
            // Negated boolean: `!requiretty` means requiretty=false.
            settings.push((stripped.trim().to_string(), "false".to_string()));
        } else {
            // Boolean flag: `requiretty` means requiretty=true.
            settings.push((part.to_string(), "true".to_string()));
        }
    }

    config.defaults.push(DefaultsDirective { scope, settings });
    Ok(())
}

/// Parse a user privilege specification line.
///
/// Format: `user host = (runas) NOPASSWD: command, command, ...`
fn parse_privilege_spec(line: &str, config: &mut SudoersConfig) -> Result<(), SudoError> {
    // Split at first `=` that is not inside parentheses.
    let eq_pos = find_eq_outside_parens(line).ok_or_else(|| {
        SudoError::ParseError(format!("missing '=' in privilege specification: {line}"))
    })?;

    let left = line[..eq_pos].trim();
    let right = line[eq_pos + 1..].trim();

    // Left side: user(s) host(s) separated by whitespace.
    // The last whitespace-separated token(s) before `=` are the hosts.
    // Simple heuristic: split by whitespace, first token is user spec,
    // remaining are hosts. If there is only one token, host is ALL.
    let left_parts: Vec<&str> = left.split_whitespace().collect();
    let (user_strs, host_strs) = if left_parts.len() >= 2 {
        let users = vec![left_parts[0]];
        let hosts: Vec<&str> = left_parts[1..].to_vec();
        (users, hosts)
    } else if left_parts.len() == 1 {
        (vec![left_parts[0]], vec!["ALL"])
    } else {
        return Err(SudoError::ParseError(
            "empty left side of privilege spec".to_string(),
        ));
    };

    let users: Vec<String> = user_strs.iter().map(|s| (*s).to_string()).collect();
    let hosts: Vec<String> = host_strs.iter().map(|s| (*s).to_string()).collect();

    // Right side: optional (runas) then tag:command pairs.
    let (runas, cmnd_str) = parse_runas_prefix(right);
    let commands = parse_cmnd_list(cmnd_str)?;

    config.privileges.push(PrivilegeSpec {
        users,
        hosts,
        runas,
        commands,
    });
    Ok(())
}

/// Find the position of `=` that is not inside parentheses.
fn find_eq_outside_parens(s: &str) -> Option<usize> {
    let mut depth = 0u32;
    for (i, c) in s.char_indices() {
        match c {
            '(' => depth = depth.saturating_add(1),
            ')' => depth = depth.saturating_sub(1),
            '=' if depth == 0 => return Some(i),
            _ => {}
        }
    }
    None
}

/// Parse the optional `(runas_user:runas_group)` prefix from the right side.
fn parse_runas_prefix(s: &str) -> (RunasSpec, &str) {
    let trimmed = s.trim();
    if !trimmed.starts_with('(') {
        return (RunasSpec::default(), trimmed);
    }

    if let Some(close) = trimmed.find(')') {
        let inner = &trimmed[1..close];
        let rest = trimmed[close + 1..].trim();

        let (user_part, group_part) = if let Some(colon) = inner.find(':') {
            (&inner[..colon], &inner[colon + 1..])
        } else {
            (inner, "")
        };

        let users: Vec<String> = user_part
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        let groups: Vec<String> = group_part
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        let runas = RunasSpec {
            users: if users.is_empty() {
                vec!["root".to_string()]
            } else {
                users
            },
            groups,
        };
        (runas, rest)
    } else {
        (RunasSpec::default(), trimmed)
    }
}

/// Parse a comma-separated command list, handling tags like NOPASSWD:, NOEXEC:, etc.
fn parse_cmnd_list(s: &str) -> Result<Vec<CmndSpec>, SudoError> {
    let mut commands = Vec::new();
    let mut nopasswd = false;
    let mut noexec = false;
    let mut setenv = false;

    for part in s.split(',') {
        let mut part = part.trim();
        if part.is_empty() {
            continue;
        }

        // Process tags (NOPASSWD:, PASSWD:, NOEXEC:, EXEC:, SETENV:, NOSETENV:).
        loop {
            if let Some(rest) = part.strip_prefix("NOPASSWD:") {
                nopasswd = true;
                part = rest.trim();
            } else if let Some(rest) = part.strip_prefix("PASSWD:") {
                nopasswd = false;
                part = rest.trim();
            } else if let Some(rest) = part.strip_prefix("NOEXEC:") {
                noexec = true;
                part = rest.trim();
            } else if let Some(rest) = part.strip_prefix("EXEC:") {
                noexec = false;
                part = rest.trim();
            } else if let Some(rest) = part.strip_prefix("SETENV:") {
                setenv = true;
                part = rest.trim();
            } else if let Some(rest) = part.strip_prefix("NOSETENV:") {
                setenv = false;
                part = rest.trim();
            } else {
                break;
            }
        }

        if part.is_empty() {
            continue;
        }

        // Split command from optional arguments.
        let (cmd, args) = if let Some(space) = part.find(' ') {
            (part[..space].trim(), part[space + 1..].trim())
        } else {
            (part, "")
        };

        commands.push(CmndSpec {
            nopasswd,
            noexec,
            setenv,
            command: cmd.to_string(),
            args: args.to_string(),
        });
    }

    Ok(commands)
}

// ============================================================================
// Sudoers syntax validation (for visudo)
// ============================================================================

/// Errors found during sudoers syntax validation.
#[derive(Debug, Clone)]
struct SyntaxError {
    line_num: usize,
    message: String,
    is_warning: bool,
}

impl fmt::Display for SyntaxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let severity = if self.is_warning { "warning" } else { "error" };
        write!(f, "line {}: {}: {}", self.line_num, severity, self.message)
    }
}

/// Validate sudoers file content, returning any syntax errors.
fn validate_sudoers(content: &str, strict: bool) -> Vec<SyntaxError> {
    let mut errors = Vec::new();
    let mut continued_line = String::new();
    let mut start_line_num = 0usize;

    for (idx, raw_line) in content.lines().enumerate() {
        let line_num = idx.wrapping_add(1);
        let trimmed = raw_line.trim();

        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        if let Some(stripped) = trimmed.strip_suffix('\\') {
            if continued_line.is_empty() {
                start_line_num = line_num;
            }
            continued_line.push_str(stripped);
            continued_line.push(' ');
            continue;
        }

        let (final_line, final_line_num) = if continued_line.is_empty() {
            (trimmed.to_string(), line_num)
        } else {
            continued_line.push_str(trimmed);
            let result = continued_line.clone();
            continued_line.clear();
            (result, start_line_num)
        };

        validate_sudoers_line(&final_line, final_line_num, strict, &mut errors);
    }

    if !continued_line.is_empty() {
        errors.push(SyntaxError {
            line_num: start_line_num,
            message: "unterminated line continuation".to_string(),
            is_warning: false,
        });
    }

    errors
}

/// Validate a single sudoers line.
fn validate_sudoers_line(
    line: &str,
    line_num: usize,
    strict: bool,
    errors: &mut Vec<SyntaxError>,
) {
    // Validate alias definitions.
    for prefix in &["User_Alias", "Host_Alias", "Cmnd_Alias", "Runas_Alias"] {
        if let Some(rest) = line.strip_prefix(prefix) {
            let rest = rest.trim();
            if !rest.contains('=') {
                errors.push(SyntaxError {
                    line_num,
                    message: format!("{prefix} missing '='"),
                    is_warning: false,
                });
                return;
            }
            let name_part = rest.split('=').next().unwrap_or("").trim();
            if name_part.is_empty() {
                errors.push(SyntaxError {
                    line_num,
                    message: format!("{prefix} has empty name"),
                    is_warning: false,
                });
            } else if !name_part
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_uppercase())
            {
                errors.push(SyntaxError {
                    line_num,
                    message: format!("{prefix} name must start with uppercase letter"),
                    is_warning: false,
                });
            }
            return;
        }
    }

    // Validate Defaults.
    if let Some(rest) = line.strip_prefix("Defaults") {
        // Just check that there is something after Defaults.
        if rest.trim().is_empty() && strict {
            errors.push(SyntaxError {
                line_num,
                message: "empty Defaults directive".to_string(),
                is_warning: !strict,
            });
        }
        return;
    }

    // Skip includes.
    if line.starts_with("#include")
        || line.starts_with("@include")
        || line.starts_with("#includedir")
        || line.starts_with("@includedir")
    {
        return;
    }

    // Privilege spec must have `=`.
    if find_eq_outside_parens(line).is_none() {
        errors.push(SyntaxError {
            line_num,
            message: "unrecognized line (missing '=' in privilege specification)".to_string(),
            is_warning: false,
        });
        return;
    }

    // Try to parse it and report any errors.
    let mut dummy = SudoersConfig::new();
    if let Err(e) = parse_privilege_spec(line, &mut dummy) {
        errors.push(SyntaxError {
            line_num,
            message: e.to_string(),
            is_warning: false,
        });
    }
}

// ============================================================================
// Authorization checking
// ============================================================================

/// Check if a user is authorized by the sudoers config to run a specific command.
fn check_authorization(
    config: &SudoersConfig,
    username: &str,
    hostname: &str,
    target_user: &str,
    target_group: &str,
    command: &str,
    user_groups: &[String],
) -> Option<CmndSpec> {
    // Iterate privileges in reverse order (last match wins, like real sudo).
    for priv_spec in config.privileges.iter().rev() {
        if !user_matches(&priv_spec.users, username, user_groups, &config.user_aliases) {
            continue;
        }
        if !host_matches(&priv_spec.hosts, hostname, &config.host_aliases) {
            continue;
        }
        if !runas_matches(&priv_spec.runas, target_user, target_group, &config.runas_aliases) {
            continue;
        }

        for cmnd in priv_spec.commands.iter().rev() {
            if command_matches(&cmnd.command, &cmnd.args, command, &config.cmnd_aliases) {
                return Some(cmnd.clone());
            }
        }
    }
    None
}

/// Check if a username matches a user specification list.
fn user_matches(
    specs: &[String],
    username: &str,
    user_groups: &[String],
    aliases: &HashMap<String, Vec<String>>,
) -> bool {
    for spec in specs {
        if spec == "ALL" {
            return true;
        }
        if spec == username {
            return true;
        }
        // %group syntax.
        if let Some(group) = spec.strip_prefix('%')
            && user_groups.iter().any(|g| g == group) {
                return true;
            }
        // Alias reference.
        if let Some(members) = aliases.get(spec.as_str()) {
            if members.iter().any(|m| m == username || m == "ALL") {
                return true;
            }
            // Check group members in alias.
            for m in members {
                if let Some(group) = m.strip_prefix('%')
                    && user_groups.iter().any(|g| g == group) {
                        return true;
                    }
            }
        }
        // Negation.
        if let Some(negated) = spec.strip_prefix('!')
            && negated == username {
                return false;
            }
    }
    false
}

/// Check if a hostname matches a host specification list.
fn host_matches(
    specs: &[String],
    hostname: &str,
    aliases: &HashMap<String, Vec<String>>,
) -> bool {
    for spec in specs {
        if spec == "ALL" {
            return true;
        }
        if spec == hostname {
            return true;
        }
        if let Some(members) = aliases.get(spec.as_str())
            && members.iter().any(|m| m == hostname || m == "ALL") {
                return true;
            }
        if let Some(negated) = spec.strip_prefix('!')
            && negated == hostname {
                return false;
            }
    }
    false
}

/// Check if target user/group matches a runas specification.
fn runas_matches(
    runas: &RunasSpec,
    target_user: &str,
    target_group: &str,
    aliases: &HashMap<String, Vec<String>>,
) -> bool {
    let user_ok = runas.users.iter().any(|u| {
        u == "ALL"
            || u == target_user
            || aliases
                .get(u.as_str())
                .is_some_and(|members| members.iter().any(|m| m == target_user || m == "ALL"))
    });

    // If no group constraint specified, only check user.
    if target_group.is_empty() || runas.groups.is_empty() {
        return user_ok;
    }

    let group_ok = runas.groups.iter().any(|g| {
        g == "ALL"
            || g == target_group
            || aliases
                .get(g.as_str())
                .is_some_and(|members| members.iter().any(|m| m == target_group || m == "ALL"))
    });

    user_ok && group_ok
}

/// Check if a command matches a command specification.
fn command_matches(
    spec_cmd: &str,
    spec_args: &str,
    actual_cmd: &str,
    aliases: &HashMap<String, Vec<String>>,
) -> bool {
    if spec_cmd == "ALL" {
        return true;
    }

    // Check aliases.
    if let Some(members) = aliases.get(spec_cmd) {
        for member in members {
            if member == "ALL" {
                return true;
            }
            // Split member into command and args.
            let (cmd, args) = if let Some(space) = member.find(' ') {
                (&member[..space], member[space + 1..].trim())
            } else {
                (member.as_str(), "")
            };
            if command_path_matches(cmd, actual_cmd)
                && (args.is_empty() || args == "*")
            {
                return true;
            }
        }
        return false;
    }

    // Negation.
    if let Some(negated) = spec_cmd.strip_prefix('!') {
        return !command_path_matches(negated, actual_cmd);
    }

    if !command_path_matches(spec_cmd, actual_cmd) {
        return false;
    }

    // If args spec is empty, allow any args.
    if spec_args.is_empty() || spec_args == "*" {
        return true;
    }

    // Otherwise, we would need to compare the actual args against spec_args.
    // For simplicity, we match if no args restriction or wildcard.
    true
}

/// Compare command paths, handling directory wildcards.
fn command_path_matches(spec: &str, actual: &str) -> bool {
    if spec == actual {
        return true;
    }
    // Wildcard: `/usr/bin/*` matches any command in `/usr/bin/`.
    if spec.ends_with("/*") {
        let dir = &spec[..spec.len() - 1];
        return actual.starts_with(dir);
    }
    // Basename match: if spec has no path separator, match basename of actual.
    if !spec.contains('/')
        && let Some(base) = actual.rsplit('/').next() {
            return base == spec;
        }
    false
}

// ============================================================================
// List user privileges
// ============================================================================

/// Format the list of privileges for a user.
fn list_privileges(
    config: &SudoersConfig,
    username: &str,
    hostname: &str,
    user_groups: &[String],
) -> String {
    let mut output = String::new();
    output.push_str(&format!(
        "User {username} may run the following commands on {hostname}:\n"
    ));

    let mut found_any = false;
    for priv_spec in &config.privileges {
        if !user_matches(&priv_spec.users, username, user_groups, &config.user_aliases) {
            continue;
        }
        if !host_matches(&priv_spec.hosts, hostname, &config.host_aliases) {
            continue;
        }

        found_any = true;
        let runas_str = format_runas(&priv_spec.runas);
        for cmnd in &priv_spec.commands {
            let tags = format_tags(cmnd);
            let cmd_str = if cmnd.args.is_empty() {
                cmnd.command.clone()
            } else {
                format!("{} {}", cmnd.command, cmnd.args)
            };
            output.push_str(&format!("    ({runas_str}) {tags}{cmd_str}\n"));
        }
    }

    if !found_any {
        output.push_str("    (none)\n");
    }

    output
}

/// Format the runas portion for display.
fn format_runas(runas: &RunasSpec) -> String {
    let user_str = runas.users.join(", ");
    if runas.groups.is_empty() {
        user_str
    } else {
        let group_str = runas.groups.join(", ");
        format!("{user_str} : {group_str}")
    }
}

/// Format command tags for display.
fn format_tags(cmnd: &CmndSpec) -> String {
    let mut tags = String::new();
    if cmnd.nopasswd {
        tags.push_str("NOPASSWD: ");
    }
    if cmnd.noexec {
        tags.push_str("NOEXEC: ");
    }
    if cmnd.setenv {
        tags.push_str("SETENV: ");
    }
    tags
}

// ============================================================================
// Timestamp management
// ============================================================================

/// Get the path to the timestamp file for a user.
fn timestamp_path(username: &str) -> PathBuf {
    PathBuf::from(TIMESTAMP_DIR).join(username)
}

/// Check if a valid timestamp exists (credential cache).
fn check_timestamp(username: &str, timeout: u64) -> bool {
    let path = timestamp_path(username);
    match fs::read_to_string(&path) {
        Ok(content) => {
            if let Some(ts_str) = content.lines().next()
                && let Ok(ts) = ts_str.trim().parse::<u64>() {
                    let now = current_epoch();
                    if timeout == u64::MAX {
                        // Never expires.
                        return true;
                    }
                    return now.saturating_sub(ts) < timeout;
                }
            false
        }
        Err(_) => false,
    }
}

/// Update the timestamp to the current time.
fn update_timestamp(username: &str) -> Result<(), SudoError> {
    let path = timestamp_path(username);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| {
            SudoError::TimestampError(format!("cannot create timestamp directory: {e}"))
        })?;
    }
    let now = current_epoch();
    fs::write(&path, format!("{now}\n")).map_err(|e| {
        SudoError::TimestampError(format!("cannot write timestamp: {e}"))
    })?;
    Ok(())
}

/// Invalidate (expire) the timestamp for a user.
fn invalidate_timestamp(username: &str) -> Result<(), SudoError> {
    let path = timestamp_path(username);
    if path.exists() {
        // Write epoch 0 to invalidate without removing.
        fs::write(&path, "0\n").map_err(|e| {
            SudoError::TimestampError(format!("cannot invalidate timestamp: {e}"))
        })?;
    }
    Ok(())
}

/// Remove the timestamp file entirely.
fn remove_timestamp(username: &str) -> Result<(), SudoError> {
    let path = timestamp_path(username);
    if path.exists() {
        fs::remove_file(&path).map_err(|e| {
            SudoError::TimestampError(format!("cannot remove timestamp: {e}"))
        })?;
    }
    Ok(())
}

/// Get current epoch time in seconds.
fn current_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ============================================================================
// Environment handling
// ============================================================================

/// Build the sanitized environment for the command execution.
fn build_environment(
    config: &SudoersConfig,
    preserve_env: bool,
    target_user: &str,
    target_home: &str,
    target_shell: &str,
    login_shell: bool,
) -> Vec<(String, String)> {
    let env_reset = config.is_default_set("env_reset")
        || config.get_default("env_reset").is_none();
    let keep_list = config.env_keep_list();
    let check_list = config.env_check_list();

    let mut env: Vec<(String, String)> = Vec::new();

    if preserve_env {
        // -E flag: preserve all current env vars except blacklisted.
        for (key, val) in std::env::vars() {
            if !ENV_BLACKLIST.iter().any(|&b| b == key) {
                env.push((key, val));
            }
        }
    } else if env_reset {
        // Default: reset environment, only keep allowed vars.
        for (key, val) in std::env::vars() {
            if keep_list.iter().any(|k| k == &key) {
                // Check for dangerous values in env_check vars.
                if check_list.iter().any(|k| k == &key)
                    && (val.contains('/') || val.contains('%')) {
                        continue; // Skip suspicious values.
                    }
                if !ENV_BLACKLIST.iter().any(|&b| b == key) {
                    env.push((key, val));
                }
            }
        }
    } else {
        // No env_reset: inherit everything except blacklisted.
        for (key, val) in std::env::vars() {
            if !ENV_BLACKLIST.iter().any(|&b| b == key) {
                env.push((key, val));
            }
        }
    }

    // Always set these.
    set_or_replace(&mut env, "USER", target_user);
    set_or_replace(&mut env, "LOGNAME", target_user);
    set_or_replace(&mut env, "SUDO_USER", &current_username());

    if login_shell {
        set_or_replace(&mut env, "HOME", target_home);
        set_or_replace(&mut env, "SHELL", target_shell);
        set_or_replace(&mut env, "PATH", "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin");
    } else {
        // Preserve HOME and SHELL from current env or set to target.
        if !env.iter().any(|(k, _)| k == "HOME") {
            env.push(("HOME".to_string(), target_home.to_string()));
        }
        if !env.iter().any(|(k, _)| k == "SHELL") {
            env.push(("SHELL".to_string(), target_shell.to_string()));
        }
    }

    // Record original command info.
    if let Ok(pwd) = std::env::current_dir() {
        set_or_replace(&mut env, "SUDO_COMMAND", "");
        set_or_replace(
            &mut env,
            "SUDO_GID",
            &format!("{}", current_gid()),
        );
        set_or_replace(
            &mut env,
            "SUDO_UID",
            &format!("{}", current_uid()),
        );
        let _ = pwd; // Acknowledged: we set SUDO_COMMAND to empty initially.
    }

    env
}

/// Set or replace an environment variable in the env list.
fn set_or_replace(env: &mut Vec<(String, String)>, key: &str, val: &str) {
    if let Some(entry) = env.iter_mut().find(|(k, _)| k == key) {
        entry.1 = val.to_string();
    } else {
        env.push((key.to_string(), val.to_string()));
    }
}

// ============================================================================
// Logging
// ============================================================================

/// Log a sudo command execution.
fn log_command(
    username: &str,
    tty: &str,
    pwd: &str,
    target_user: &str,
    command: &str,
    result: &str,
) {
    let timestamp = format_timestamp(current_epoch());
    let log_line = format!(
        "{timestamp} : {username} : TTY={tty} ; PWD={pwd} ; USER={target_user} ; COMMAND={command} ; RESULT={result}\n"
    );

    // Attempt to write — failure is non-fatal.
    if let Some(parent) = Path::new(SUDO_LOG_PATH).parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(mut f) = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(SUDO_LOG_PATH)
    {
        let _ = f.write_all(log_line.as_bytes());
    }
}

/// Format an epoch timestamp as a human-readable string.
fn format_timestamp(epoch: u64) -> String {
    // Simple epoch-based formatting (OurOS will have its own time formatting).
    // Format: YYYY-MM-DD HH:MM:SS (approximate, using basic calculation).
    let secs_per_minute = 60u64;
    let secs_per_hour = 3600u64;
    let secs_per_day = 86400u64;

    let days = epoch / secs_per_day;
    let remaining = epoch % secs_per_day;
    let hours = remaining / secs_per_hour;
    let remaining = remaining % secs_per_hour;
    let minutes = remaining / secs_per_minute;
    let seconds = remaining % secs_per_minute;

    // Approximate date from days since epoch (1970-01-01).
    let (year, month, day) = days_to_date(days);

    format!(
        "{year:04}-{month:02}-{day:02} {hours:02}:{minutes:02}:{seconds:02}"
    )
}

/// Convert days since epoch to (year, month, day).
fn days_to_date(mut days: u64) -> (u64, u64, u64) {
    let mut year = 1970u64;

    loop {
        let days_in_year = if is_leap_year(year) { 366 } else { 365 };
        if days < days_in_year {
            break;
        }
        days -= days_in_year;
        year += 1;
    }

    let leap = is_leap_year(year);
    let month_days: [u64; 12] = [
        31,
        if leap { 29 } else { 28 },
        31, 30, 31, 30, 31, 31, 30, 31, 30, 31,
    ];

    let mut month = 1u64;
    for &md in &month_days {
        if days < md {
            break;
        }
        days -= md;
        month += 1;
    }

    (year, month, days + 1)
}

/// Check if a year is a leap year.
fn is_leap_year(year: u64) -> bool {
    (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400)
}

// ============================================================================
// Session I/O recording and replay
// ============================================================================

/// A recorded session entry.
#[derive(Debug, Clone)]
struct SessionEntry {
    id: String,
    user: String,
    target_user: String,
    command: String,
    timestamp: u64,
    _tty: String,
}

/// List recorded sessions from the I/O log directory.
fn list_sessions(io_dir: &str) -> Vec<SessionEntry> {
    let mut sessions = Vec::new();
    let dir = Path::new(io_dir);
    if !dir.is_dir() {
        return sessions;
    }

    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return sessions,
    };

    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let session_id = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };

        // Read the log file.
        let log_path = path.join("log");
        let log_content = match fs::read_to_string(&log_path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let mut user = String::new();
        let mut target_user = String::new();
        let mut command = String::new();
        let mut timestamp = 0u64;
        let mut tty = String::new();

        for line in log_content.lines() {
            if let Some(val) = line.strip_prefix("user=") {
                user = val.trim().to_string();
            } else if let Some(val) = line.strip_prefix("runas_user=") {
                target_user = val.trim().to_string();
            } else if let Some(val) = line.strip_prefix("command=") {
                command = val.trim().to_string();
            } else if let Some(val) = line.strip_prefix("timestamp=") {
                timestamp = val.trim().parse().unwrap_or(0);
            } else if let Some(val) = line.strip_prefix("tty=") {
                tty = val.trim().to_string();
            }
        }

        sessions.push(SessionEntry {
            id: session_id,
            user,
            target_user,
            command,
            timestamp,
            _tty: tty,
        });
    }

    sessions.sort_by_key(|b| std::cmp::Reverse(b.timestamp));
    sessions
}

/// Replay a recorded session.
fn replay_session(
    io_dir: &str,
    session_id: &str,
    speed_factor: f64,
) -> Result<(), SudoError> {
    let session_dir = Path::new(io_dir).join(session_id);
    if !session_dir.is_dir() {
        return Err(SudoError::IoError(format!(
            "session directory not found: {}",
            session_dir.display()
        )));
    }

    // Read timing file.
    let timing_path = session_dir.join("timing");
    let timing_content = fs::read_to_string(&timing_path).map_err(|e| {
        SudoError::IoError(format!("cannot read timing file: {e}"))
    })?;

    // Read stdout data.
    let stdout_path = session_dir.join("stdout");
    let stdout_data = fs::read(&stdout_path).map_err(|e| {
        SudoError::IoError(format!("cannot read stdout file: {e}"))
    })?;

    // Read log info.
    let log_path = session_dir.join("log");
    if let Ok(log_content) = fs::read_to_string(&log_path) {
        eprintln!("Replaying session {session_id}:");
        for line in log_content.lines() {
            eprintln!("  {line}");
        }
        eprintln!();
    }

    // Parse and replay timing entries.
    // Format: TYPE SECONDS BYTES
    // TYPE: 1 = stdout, 2 = stderr, 3 = stdin
    let mut offset = 0usize;
    let stdout = io::stdout();
    let mut out = stdout.lock();

    for line in timing_content.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 3 {
            continue;
        }

        let stream_type: u32 = match parts[0].parse() {
            Ok(t) => t,
            Err(_) => continue,
        };
        let delay_secs: f64 = match parts[1].parse() {
            Ok(d) => d,
            Err(_) => continue,
        };
        let nbytes: usize = match parts[2].parse() {
            Ok(n) => n,
            Err(_) => continue,
        };

        // Apply speed factor to delay.
        let adjusted_delay = delay_secs / speed_factor;
        if adjusted_delay > 0.001 {
            // Sleep for the adjusted delay.
            // On OurOS, this would use the real sleep syscall.
            // For now, spin-wait approximation.
            let target = current_epoch_nanos().saturating_add((adjusted_delay * 1_000_000_000.0) as u64);
            while current_epoch_nanos() < target {
                std::hint::spin_loop();
            }
        }

        // Only replay stdout (type 1).
        if stream_type == 1 {
            let end = offset.saturating_add(nbytes).min(stdout_data.len());
            if offset < stdout_data.len() {
                let _ = out.write_all(&stdout_data[offset..end]);
                let _ = out.flush();
            }
            offset = end;
        } else {
            offset = offset.saturating_add(nbytes);
        }
    }

    eprintln!("\nReplay finished.");
    Ok(())
}

/// Get current time in nanoseconds (approximate).
fn current_epoch_nanos() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

// ============================================================================
// Prompt and authentication
// ============================================================================

/// Expand prompt template variables.
fn expand_prompt(template: &str, username: &str, hostname: &str, target_user: &str) -> String {
    let mut result = template.to_string();
    // Replace each known placeholder.
    result = result.replace("%u", username);
    result = result.replace("%U", target_user);
    result = result.replace("%h", hostname);
    result = result.replace("%H", hostname);
    result = result.replace("%%", "%");
    result
}

/// Prompt for a password (reads from /dev/tty or stdin).
fn prompt_password(prompt: &str) -> Result<String, SudoError> {
    eprint!("{prompt}");
    let _ = io::stderr().flush();

    let mut password = String::new();

    // Try /dev/tty first, fall back to stdin.
    let result = if let Ok(mut tty) = fs::File::open("/dev/tty") {
        tty.read_to_string(&mut password)
    } else {
        io::stdin().read_line(&mut password).map(|_| password.len())
    };

    match result {
        Ok(_) => {
            // Remove trailing newline.
            if password.ends_with('\n') {
                password.pop();
            }
            if password.ends_with('\r') {
                password.pop();
            }
            Ok(password)
        }
        Err(e) => Err(SudoError::AuthError(format!(
            "failed to read password: {e}"
        ))),
    }
}

/// Authenticate the user. Returns Ok(()) on success.
///
/// On OurOS, this would use the PAM equivalent or shadow password file.
/// For now, this checks against `/etc/shadow` (simplified).
fn authenticate(username: &str, _password: &str) -> Result<(), SudoError> {
    // In OurOS, authentication will be handled by the auth service via IPC.
    // This stub checks if the user exists in the user database.
    let user_db = Path::new("/etc/users.yaml");
    if !user_db.exists() {
        // If no user database, allow (development/single-user mode).
        return Ok(());
    }

    let content = fs::read_to_string(user_db).map_err(|e| {
        SudoError::AuthError(format!("cannot read user database: {e}"))
    })?;

    // Simple check: see if the username appears in the database.
    if content.contains(&format!("name: {username}"))
        || content.contains(&format!("name: \"{username}\""))
    {
        // In a real implementation, we would verify the password hash.
        Ok(())
    } else {
        Err(SudoError::AuthError(format!(
            "user {username} not found in user database"
        )))
    }
}

// ============================================================================
// Platform helpers (OurOS stubs)
// ============================================================================

/// Get the current username.
fn current_username() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("LOGNAME"))
        .unwrap_or_else(|_| "unknown".to_string())
}

/// Get the current hostname.
fn current_hostname() -> String {
    // Try /etc/hostname first.
    if let Ok(name) = fs::read_to_string("/etc/hostname") {
        let name = name.trim().to_string();
        if !name.is_empty() {
            return name;
        }
    }
    std::env::var("HOSTNAME").unwrap_or_else(|_| "localhost".to_string())
}

/// Get the current uid (stub — reads from env or returns 1000).
fn current_uid() -> u32 {
    std::env::var("UID")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1000)
}

/// Get the current gid (stub — reads from env or returns 1000).
fn current_gid() -> u32 {
    std::env::var("GID")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1000)
}

/// Get the current tty name.
fn current_tty() -> String {
    std::env::var("TTY").unwrap_or_else(|_| "unknown".to_string())
}

/// Get user groups for a username (stub).
fn get_user_groups(username: &str) -> Vec<String> {
    // In OurOS, read from /etc/users.yaml.
    let mut groups = vec![username.to_string()];
    if let Ok(content) = fs::read_to_string("/etc/users.yaml") {
        // Simple parser: find the user's groups line.
        let mut in_user = false;
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed == format!("name: {username}")
                || trimmed == format!("name: \"{username}\"")
            {
                in_user = true;
                continue;
            }
            if in_user && trimmed.starts_with("groups:") {
                // Parse YAML list: groups: [wheel, admin]
                if let Some(list_start) = trimmed.find('[')
                    && let Some(list_end) = trimmed.find(']') {
                        let list = &trimmed[list_start + 1..list_end];
                        for g in list.split(',') {
                            let g = g.trim().trim_matches('"').trim_matches('\'');
                            if !g.is_empty() && !groups.contains(&g.to_string()) {
                                groups.push(g.to_string());
                            }
                        }
                    }
                break;
            }
            if in_user && !trimmed.is_empty() && !trimmed.starts_with('-') && !trimmed.starts_with(' ') {
                break; // Moved to next user entry.
            }
        }
    }
    // Common default groups.
    if username == "root" && !groups.contains(&"wheel".to_string()) {
        groups.push("wheel".to_string());
    }
    groups
}

/// Get target user info (home, shell).
fn get_user_info(username: &str) -> (String, String) {
    if username == "root" {
        return ("/root".to_string(), "/bin/sh".to_string());
    }
    // Try to read from /etc/users.yaml.
    if let Ok(content) = fs::read_to_string("/etc/users.yaml") {
        let mut in_user = false;
        let mut home = format!("/home/{username}");
        let mut shell = "/bin/sh".to_string();

        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed == format!("name: {username}")
                || trimmed == format!("name: \"{username}\"")
            {
                in_user = true;
                continue;
            }
            if in_user {
                if let Some(val) = trimmed.strip_prefix("home:") {
                    home = val.trim().trim_matches('"').to_string();
                } else if let Some(val) = trimmed.strip_prefix("shell:") {
                    shell = val.trim().trim_matches('"').to_string();
                } else if !trimmed.is_empty()
                    && !trimmed.starts_with('-')
                    && !trimmed.starts_with(' ')
                    && !trimmed.starts_with("uid:")
                    && !trimmed.starts_with("gid:")
                    && !trimmed.starts_with("groups:")
                {
                    break;
                }
            }
        }
        return (home, shell);
    }
    (format!("/home/{username}"), "/bin/sh".to_string())
}

// ============================================================================
// File locking for visudo
// ============================================================================

/// Simple file-based lock.
fn acquire_lock(path: &Path) -> Result<PathBuf, SudoError> {
    let lock_path = path.with_extension("lck");
    if lock_path.exists() {
        // Check if the lock is stale (older than 5 minutes).
        if let Ok(meta) = fs::metadata(&lock_path)
            && let Ok(modified) = meta.modified()
                && let Ok(elapsed) = modified.elapsed()
                    && elapsed.as_secs() < 300 {
                        return Err(SudoError::LockError(format!(
                            "{} is locked by another process",
                            path.display()
                        )));
                    }
                    // Stale lock — remove it.
    }

    // Create the lock file with our PID.
    fs::write(&lock_path, format!("{}\n", std::process::id())).map_err(|e| {
        SudoError::LockError(format!("cannot create lock file: {e}"))
    })?;

    Ok(lock_path)
}

/// Release a file lock.
fn release_lock(lock_path: &Path) {
    let _ = fs::remove_file(lock_path);
}

// ============================================================================
// JSON escaping
// ============================================================================

/// Escape a string for safe inclusion in JSON.
/// Used in structured log output; retained for future JSON-lines logging.
#[allow(dead_code)]
fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
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

// ============================================================================
// Sudo options
// ============================================================================

/// Parsed command-line options for the sudo personality.
#[derive(Debug)]
struct SudoOpts {
    target_user: String,
    target_group: String,
    login_shell: bool,
    shell: bool,
    list: bool,
    validate: bool,
    invalidate: bool,
    remove_timestamp: bool,
    non_interactive: bool,
    background: bool,
    edit_mode: bool,
    preserve_env: bool,
    prompt: String,
    command: Vec<String>,
}

impl Default for SudoOpts {
    fn default() -> Self {
        Self {
            target_user: "root".to_string(),
            target_group: String::new(),
            login_shell: false,
            shell: false,
            list: false,
            validate: false,
            invalidate: false,
            remove_timestamp: false,
            non_interactive: false,
            background: false,
            edit_mode: false,
            preserve_env: false,
            prompt: DEFAULT_PROMPT.to_string(),
            command: Vec::new(),
        }
    }
}

/// Parse sudo command-line arguments.
fn parse_sudo_args(args: &[String]) -> Result<SudoOpts, SudoError> {
    let mut opts = SudoOpts::default();
    let mut i = 0;
    let mut end_of_opts = false;

    while i < args.len() {
        if end_of_opts {
            opts.command.push(args[i].clone());
            i += 1;
            continue;
        }

        let arg = &args[i];

        if arg == "--" {
            end_of_opts = true;
            i += 1;
            continue;
        }

        if !arg.starts_with('-') {
            // First non-option argument starts the command.
            opts.command.extend(args[i..].iter().cloned());
            break;
        }

        // Handle combined short flags like -inE.
        if arg.starts_with('-') && !arg.starts_with("--") && arg.len() > 2 {
            // Could be combined flags or a flag with value.
            let flags = &arg[1..];
            let mut j = 0;
            let flag_bytes = flags.as_bytes();
            while j < flag_bytes.len() {
                match flag_bytes[j] {
                    b'u' => {
                        // -u takes the rest or next arg as value.
                        let rest = &flags[j + 1..];
                        if !rest.is_empty() {
                            opts.target_user = rest.to_string();
                        } else {
                            i += 1;
                            if i >= args.len() {
                                return Err(SudoError::UsageError(
                                    "-u requires an argument".to_string(),
                                ));
                            }
                            opts.target_user = args[i].clone();
                        }
                        j = flag_bytes.len(); // Consumed rest.
                    }
                    b'g' => {
                        let rest = &flags[j + 1..];
                        if !rest.is_empty() {
                            opts.target_group = rest.to_string();
                        } else {
                            i += 1;
                            if i >= args.len() {
                                return Err(SudoError::UsageError(
                                    "-g requires an argument".to_string(),
                                ));
                            }
                            opts.target_group = args[i].clone();
                        }
                        j = flag_bytes.len();
                    }
                    b'p' => {
                        let rest = &flags[j + 1..];
                        if !rest.is_empty() {
                            opts.prompt = rest.to_string();
                        } else {
                            i += 1;
                            if i >= args.len() {
                                return Err(SudoError::UsageError(
                                    "-p requires an argument".to_string(),
                                ));
                            }
                            opts.prompt = args[i].clone();
                        }
                        j = flag_bytes.len();
                    }
                    b'i' => { opts.login_shell = true; j += 1; }
                    b's' => { opts.shell = true; j += 1; }
                    b'l' => { opts.list = true; j += 1; }
                    b'v' => { opts.validate = true; j += 1; }
                    b'k' => { opts.invalidate = true; j += 1; }
                    b'K' => { opts.remove_timestamp = true; j += 1; }
                    b'n' => { opts.non_interactive = true; j += 1; }
                    b'b' => { opts.background = true; j += 1; }
                    b'e' => { opts.edit_mode = true; j += 1; }
                    b'E' => { opts.preserve_env = true; j += 1; }
                    _ => {
                        return Err(SudoError::UsageError(format!(
                            "unknown option: -{}", flags.chars().nth(j).unwrap_or('?')
                        )));
                    }
                }
            }
            i += 1;
            continue;
        }

        match arg.as_str() {
            "-u" => {
                i += 1;
                if i >= args.len() {
                    return Err(SudoError::UsageError("-u requires an argument".to_string()));
                }
                opts.target_user = args[i].clone();
            }
            "-g" => {
                i += 1;
                if i >= args.len() {
                    return Err(SudoError::UsageError("-g requires an argument".to_string()));
                }
                opts.target_group = args[i].clone();
            }
            "-p" => {
                i += 1;
                if i >= args.len() {
                    return Err(SudoError::UsageError("-p requires an argument".to_string()));
                }
                opts.prompt = args[i].clone();
            }
            "-i" => opts.login_shell = true,
            "-s" => opts.shell = true,
            "-l" => opts.list = true,
            "-v" => opts.validate = true,
            "-k" => opts.invalidate = true,
            "-K" => opts.remove_timestamp = true,
            "-n" => opts.non_interactive = true,
            "-b" => opts.background = true,
            "-e" => opts.edit_mode = true,
            "-E" => opts.preserve_env = true,
            other => {
                return Err(SudoError::UsageError(format!("unknown option: {other}")));
            }
        }

        i += 1;
    }

    Ok(opts)
}

// ============================================================================
// Visudo options
// ============================================================================

/// Parsed command-line options for the visudo personality.
#[derive(Debug)]
struct VisudoOpts {
    check_only: bool,
    file: String,
    strict: bool,
}

impl Default for VisudoOpts {
    fn default() -> Self {
        Self {
            check_only: false,
            file: SUDOERS_PATH.to_string(),
            strict: false,
        }
    }
}

/// Parse visudo command-line arguments.
fn parse_visudo_args(args: &[String]) -> Result<VisudoOpts, SudoError> {
    let mut opts = VisudoOpts::default();
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "-c" => opts.check_only = true,
            "-s" => opts.strict = true,
            "-f" => {
                i += 1;
                if i >= args.len() {
                    return Err(SudoError::UsageError("-f requires an argument".to_string()));
                }
                opts.file = args[i].clone();
            }
            other if other.starts_with('-') => {
                return Err(SudoError::UsageError(format!(
                    "unknown option: {other}"
                )));
            }
            _ => {
                return Err(SudoError::UsageError(format!(
                    "unexpected argument: {}",
                    args[i]
                )));
            }
        }
        i += 1;
    }

    Ok(opts)
}

// ============================================================================
// Sudoreplay options
// ============================================================================

/// Parsed command-line options for the sudoreplay personality.
#[derive(Debug)]
struct SudoreplayOpts {
    list: bool,
    directory: String,
    speed_factor: f64,
    session_id: Option<String>,
}

impl Default for SudoreplayOpts {
    fn default() -> Self {
        Self {
            list: false,
            directory: SUDO_IO_DIR.to_string(),
            speed_factor: 1.0,
            session_id: None,
        }
    }
}

/// Parse sudoreplay command-line arguments.
fn parse_sudoreplay_args(args: &[String]) -> Result<SudoreplayOpts, SudoError> {
    let mut opts = SudoreplayOpts::default();
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "-l" => opts.list = true,
            "-d" => {
                i += 1;
                if i >= args.len() {
                    return Err(SudoError::UsageError("-d requires an argument".to_string()));
                }
                opts.directory = args[i].clone();
            }
            "-s" => {
                i += 1;
                if i >= args.len() {
                    return Err(SudoError::UsageError("-s requires an argument".to_string()));
                }
                opts.speed_factor = args[i].parse::<f64>().map_err(|_| {
                    SudoError::UsageError("invalid speed factor".to_string())
                })?;
                if opts.speed_factor <= 0.0 {
                    return Err(SudoError::UsageError(
                        "speed factor must be positive".to_string(),
                    ));
                }
            }
            other if other.starts_with('-') => {
                return Err(SudoError::UsageError(format!(
                    "unknown option: {other}"
                )));
            }
            other => {
                opts.session_id = Some(other.to_string());
            }
        }
        i += 1;
    }

    Ok(opts)
}

// ============================================================================
// Usage messages
// ============================================================================

fn print_sudo_usage() {
    eprintln!("usage: sudo [-u user] [-g group] [-i] [-s] [-b] [-n] [-E] [-p prompt] [--] command [args...]");
    eprintln!("       sudo -l               List user's privileges");
    eprintln!("       sudo -v               Validate / extend timestamp");
    eprintln!("       sudo -k               Invalidate timestamp");
    eprintln!("       sudo -K               Remove timestamp entirely");
    eprintln!("       sudo -e file...       Edit files (sudoedit mode)");
}

fn print_visudo_usage() {
    eprintln!("usage: visudo [-c] [-f file] [-s]");
    eprintln!("       -c          Check syntax only");
    eprintln!("       -f file     Edit alternate sudoers file");
    eprintln!("       -s          Strict mode (error on warnings)");
}

fn print_sudoreplay_usage() {
    eprintln!("usage: sudoreplay [-l] [-d dir] [-s speed_factor] [session_id]");
    eprintln!("       -l          List recorded sessions");
    eprintln!("       -d dir      Session I/O directory");
    eprintln!("       -s factor   Playback speed factor");
}

// ============================================================================
// Personality entry points
// ============================================================================

/// Main entry point for the `sudo` personality.
fn run_sudo(args: &[String]) -> i32 {
    let opts = match parse_sudo_args(args) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("sudo: {e}");
            print_sudo_usage();
            return 1;
        }
    };

    // Handle -e (edit mode) by delegating to sudoedit.
    if opts.edit_mode {
        return run_sudoedit(&opts.command);
    }

    let username = current_username();
    let hostname = current_hostname();

    // Handle -K (remove timestamp entirely).
    if opts.remove_timestamp {
        if let Err(e) = remove_timestamp(&username) {
            eprintln!("sudo: {e}");
            return 1;
        }
        return 0;
    }

    // Handle -k (invalidate timestamp).
    if opts.invalidate {
        if let Err(e) = invalidate_timestamp(&username) {
            eprintln!("sudo: {e}");
            return 1;
        }
        // If there is also a command, continue executing it.
        if opts.command.is_empty() && !opts.validate && !opts.list {
            return 0;
        }
    }

    // Load sudoers.
    let config = match load_sudoers() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("sudo: {e}");
            return 1;
        }
    };

    let user_groups = get_user_groups(&username);

    // Handle -l (list privileges).
    if opts.list {
        let listing = list_privileges(&config, &username, &hostname, &user_groups);
        print!("{listing}");
        return 0;
    }

    // Handle -v (validate / extend timestamp).
    if opts.validate {
        let timeout = config.timestamp_timeout();
        if !check_timestamp(&username, timeout) {
            if opts.non_interactive {
                eprintln!("sudo: a password is required");
                return 1;
            }
            let prompt_str = expand_prompt(&opts.prompt, &username, &hostname, &opts.target_user);
            match prompt_password(&prompt_str) {
                Ok(pw) => {
                    if let Err(e) = authenticate(&username, &pw) {
                        eprintln!("sudo: {e}");
                        log_command(
                            &username,
                            &current_tty(),
                            &std::env::current_dir()
                                .map(|p| p.display().to_string())
                                .unwrap_or_else(|_| "unknown".to_string()),
                            &opts.target_user,
                            "(validate)",
                            "AUTH_FAILURE",
                        );
                        return 1;
                    }
                }
                Err(e) => {
                    eprintln!("sudo: {e}");
                    return 1;
                }
            }
        }
        if let Err(e) = update_timestamp(&username) {
            eprintln!("sudo: {e}");
            return 1;
        }
        return 0;
    }

    // Must have a command (unless -i or -s without args means run a shell).
    if opts.command.is_empty() && !opts.login_shell && !opts.shell {
        print_sudo_usage();
        return 1;
    }

    // Determine the actual command.
    let (target_home, target_shell) = get_user_info(&opts.target_user);
    let effective_command = if opts.command.is_empty() {
        // -i or -s without command: run the target user's shell.
        vec![target_shell.clone()]
    } else {
        opts.command.clone()
    };

    let command_str = effective_command.join(" ");

    // Check authorization.
    let auth_result = check_authorization(
        &config,
        &username,
        &hostname,
        &opts.target_user,
        &opts.target_group,
        &effective_command[0],
        &user_groups,
    );

    let cmnd_spec = match auth_result {
        Some(spec) => spec,
        None => {
            eprintln!(
                "sudo: {username} is not allowed to run '{}' as {} on {hostname}",
                command_str, opts.target_user
            );
            log_command(
                &username,
                &current_tty(),
                &std::env::current_dir()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|_| "unknown".to_string()),
                &opts.target_user,
                &command_str,
                "NOT_ALLOWED",
            );
            return 1;
        }
    };

    // Authenticate if required.
    let timeout = config.timestamp_timeout();
    if !cmnd_spec.nopasswd && !check_timestamp(&username, timeout) {
        // Root does not need a password.
        if current_uid() != 0 {
            if opts.non_interactive {
                eprintln!("sudo: a password is required");
                log_command(
                    &username,
                    &current_tty(),
                    &std::env::current_dir()
                        .map(|p| p.display().to_string())
                        .unwrap_or_else(|_| "unknown".to_string()),
                    &opts.target_user,
                    &command_str,
                    "AUTH_REQUIRED",
                );
                return 1;
            }

            let prompt_str =
                expand_prompt(&opts.prompt, &username, &hostname, &opts.target_user);
            match prompt_password(&prompt_str) {
                Ok(pw) => {
                    if let Err(e) = authenticate(&username, &pw) {
                        eprintln!("sudo: {e}");
                        log_command(
                            &username,
                            &current_tty(),
                            &std::env::current_dir()
                                .map(|p| p.display().to_string())
                                .unwrap_or_else(|_| "unknown".to_string()),
                            &opts.target_user,
                            &command_str,
                            "AUTH_FAILURE",
                        );
                        return 1;
                    }
                }
                Err(e) => {
                    eprintln!("sudo: {e}");
                    return 1;
                }
            }

            // Update timestamp on successful auth.
            let _ = update_timestamp(&username);
        }
    }

    // Build environment.
    let _env = build_environment(
        &config,
        opts.preserve_env || cmnd_spec.setenv,
        &opts.target_user,
        &target_home,
        &target_shell,
        opts.login_shell,
    );

    // Log the command.
    log_command(
        &username,
        &current_tty(),
        &std::env::current_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| "unknown".to_string()),
        &opts.target_user,
        &command_str,
        "ALLOWED",
    );

    // Execute the command.
    // On OurOS, this would use exec() syscall to replace the process.
    // For now, we simulate with std::process::Command.
    let mut cmd = process::Command::new(&effective_command[0]);
    if effective_command.len() > 1 {
        cmd.args(&effective_command[1..]);
    }

    // Set the environment.
    cmd.env_clear();
    for (key, val) in &_env {
        cmd.env(key, val);
    }

    if opts.login_shell {
        cmd.current_dir(&target_home);
    }

    // If -i, wrap in shell -l.
    let mut cmd = if opts.login_shell && !opts.command.is_empty() {
        let mut shell_cmd = process::Command::new(&target_shell);
        shell_cmd.arg("-l").arg("-c").arg(&command_str);
        shell_cmd.env_clear();
        for (key, val) in &_env {
            shell_cmd.env(key, val);
        }
        shell_cmd.current_dir(&target_home);
        shell_cmd
    } else if opts.shell && !opts.command.is_empty() {
        let mut shell_cmd = process::Command::new(&target_shell);
        shell_cmd.arg("-c").arg(&command_str);
        shell_cmd.env_clear();
        for (key, val) in &_env {
            shell_cmd.env(key, val);
        }
        shell_cmd
    } else {
        cmd
    };

    match cmd.status() {
        Ok(status) => status.code().unwrap_or(1),
        Err(e) => {
            eprintln!("sudo: unable to execute {}: {e}", effective_command[0]);
            1
        }
    }
}

/// Load and parse the sudoers file.
fn load_sudoers() -> Result<SudoersConfig, SudoError> {
    let content = fs::read_to_string(SUDOERS_PATH).map_err(|e| {
        SudoError::InvalidConfig(format!("cannot read {SUDOERS_PATH}: {e}"))
    })?;
    parse_sudoers(&content)
}

/// Main entry point for the `sudoedit` personality.
fn run_sudoedit(files: &[String]) -> i32 {
    if files.is_empty() {
        eprintln!("sudoedit: no files specified");
        eprintln!("usage: sudoedit file [file ...]");
        return 1;
    }

    let username = current_username();
    let hostname = current_hostname();
    let user_groups = get_user_groups(&username);

    // Load sudoers to check authorization.
    let config = match load_sudoers() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("sudoedit: {e}");
            return 1;
        }
    };

    // Check authorization for sudoedit.
    let auth_result = check_authorization(
        &config,
        &username,
        &hostname,
        "root",
        "",
        "sudoedit",
        &user_groups,
    );

    if auth_result.is_none() {
        // Also check for the specific files.
        for file in files {
            let result = check_authorization(
                &config,
                &username,
                &hostname,
                "root",
                "",
                file,
                &user_groups,
            );
            if result.is_none() {
                eprintln!(
                    "sudoedit: {username} is not allowed to edit {file} on {hostname}"
                );
                return 1;
            }
        }
    }

    let editor = std::env::var("SUDO_EDITOR")
        .or_else(|_| std::env::var("VISUAL"))
        .or_else(|_| std::env::var("EDITOR"))
        .unwrap_or_else(|_| DEFAULT_EDITOR.to_string());

    let mut exit_code = 0;

    for file in files {
        let original_path = Path::new(file);

        // Create a temporary copy.
        let temp_path = PathBuf::from(format!(
            "/tmp/sudoedit-{}-{}",
            std::process::id(),
            original_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("file")
        ));

        // Copy original to temp (if it exists).
        if original_path.exists() {
            if let Err(e) = fs::copy(original_path, &temp_path) {
                eprintln!("sudoedit: cannot copy {file} to temp: {e}");
                exit_code = 1;
                continue;
            }
        } else {
            // Create empty temp file.
            if let Err(e) = fs::write(&temp_path, "") {
                eprintln!("sudoedit: cannot create temp file: {e}");
                exit_code = 1;
                continue;
            }
        }

        // Launch editor on the temp file.
        let status = process::Command::new(&editor)
            .arg(temp_path.display().to_string())
            .status();

        match status {
            Ok(s) if s.success() => {
                // Copy edited temp back to original.
                if let Err(e) = fs::copy(&temp_path, original_path) {
                    eprintln!("sudoedit: cannot write back to {file}: {e}");
                    exit_code = 1;
                }
            }
            Ok(s) => {
                eprintln!(
                    "sudoedit: editor exited with status {}",
                    s.code().unwrap_or(-1)
                );
                exit_code = 1;
            }
            Err(e) => {
                eprintln!("sudoedit: cannot run editor '{editor}': {e}");
                exit_code = 1;
            }
        }

        // Clean up temp file.
        let _ = fs::remove_file(&temp_path);
    }

    log_command(
        &username,
        &current_tty(),
        &std::env::current_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| "unknown".to_string()),
        "root",
        &format!("sudoedit {}", files.join(" ")),
        if exit_code == 0 { "SUCCESS" } else { "FAILURE" },
    );

    exit_code
}

/// Main entry point for the `visudo` personality.
fn run_visudo(args: &[String]) -> i32 {
    let opts = match parse_visudo_args(args) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("visudo: {e}");
            print_visudo_usage();
            return 1;
        }
    };

    let file_path = Path::new(&opts.file);

    // Check-only mode.
    if opts.check_only {
        let content = match fs::read_to_string(file_path) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("visudo: cannot read {}: {e}", opts.file);
                return 1;
            }
        };

        let errors = validate_sudoers(&content, opts.strict);
        if errors.is_empty() {
            println!("{} parsed OK", opts.file);
            return 0;
        }

        for err in &errors {
            eprintln!("visudo: {}: {err}", opts.file);
        }

        let fatal_count = errors.iter().filter(|e| !e.is_warning).count();
        if fatal_count > 0 {
            return 1;
        }
        if opts.strict {
            return 1;
        }
        println!("{} parsed with warnings", opts.file);
        return 0;
    }

    // Editing mode.
    // Acquire lock.
    let lock_path = match acquire_lock(file_path) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("visudo: {e}");
            return 1;
        }
    };

    // Read current content.
    let original_content = fs::read_to_string(file_path).unwrap_or_default();

    // Create temp file.
    let temp_path = PathBuf::from(format!("/tmp/visudo-{}", std::process::id()));
    if let Err(e) = fs::write(&temp_path, &original_content) {
        eprintln!("visudo: cannot create temp file: {e}");
        release_lock(&lock_path);
        return 1;
    }

    let editor = std::env::var("SUDO_EDITOR")
        .or_else(|_| std::env::var("VISUAL"))
        .or_else(|_| std::env::var("EDITOR"))
        .unwrap_or_else(|_| DEFAULT_EDITOR.to_string());

    // Edit loop: keep re-editing until valid or user quits.
    loop {
        let status = process::Command::new(&editor)
            .arg(temp_path.display().to_string())
            .status();

        match status {
            Ok(s) if !s.success() => {
                eprintln!(
                    "visudo: editor exited with status {}",
                    s.code().unwrap_or(-1)
                );
                let _ = fs::remove_file(&temp_path);
                release_lock(&lock_path);
                return 1;
            }
            Err(e) => {
                eprintln!("visudo: cannot run editor '{editor}': {e}");
                let _ = fs::remove_file(&temp_path);
                release_lock(&lock_path);
                return 1;
            }
            _ => {}
        }

        // Read edited content.
        let new_content = match fs::read_to_string(&temp_path) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("visudo: cannot read temp file: {e}");
                release_lock(&lock_path);
                return 1;
            }
        };

        // Validate.
        let errors = validate_sudoers(&new_content, opts.strict);
        let fatal_errors: Vec<&SyntaxError> = errors.iter().filter(|e| !e.is_warning).collect();

        if fatal_errors.is_empty() {
            // Valid — write back.
            if let Err(e) = fs::write(file_path, &new_content) {
                eprintln!("visudo: cannot write {}: {e}", opts.file);
                let _ = fs::remove_file(&temp_path);
                release_lock(&lock_path);
                return 1;
            }

            // Set permissions (sudoers should be 0440).
            // On OurOS, this would use chmod syscall.

            let _ = fs::remove_file(&temp_path);
            release_lock(&lock_path);
            return 0;
        }

        // Report errors and ask what to do.
        for err in &fatal_errors {
            eprintln!("visudo: {}: {err}", opts.file);
        }
        eprint!("What now? (e)dit again, e(x)it without saving, (Q)uit and save: ");
        let _ = io::stderr().flush();

        let mut response = String::new();
        if io::stdin().read_line(&mut response).is_err() {
            let _ = fs::remove_file(&temp_path);
            release_lock(&lock_path);
            return 1;
        }

        match response.trim() {
            "e" | "E" => continue,
            "x" | "X" => {
                let _ = fs::remove_file(&temp_path);
                release_lock(&lock_path);
                return 0;
            }
            "Q" => {
                // Save despite errors.
                if let Err(e) = fs::write(file_path, &new_content) {
                    eprintln!("visudo: cannot write {}: {e}", opts.file);
                    let _ = fs::remove_file(&temp_path);
                    release_lock(&lock_path);
                    return 1;
                }
                let _ = fs::remove_file(&temp_path);
                release_lock(&lock_path);
                return 0;
            }
            _ => continue,
        }
    }
}

/// Main entry point for the `sudoreplay` personality.
fn run_sudoreplay(args: &[String]) -> i32 {
    let opts = match parse_sudoreplay_args(args) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("sudoreplay: {e}");
            print_sudoreplay_usage();
            return 1;
        }
    };

    // List mode.
    if opts.list {
        let sessions = list_sessions(&opts.directory);
        if sessions.is_empty() {
            println!("No recorded sessions found in {}", opts.directory);
            return 0;
        }

        println!(
            "{:<12} {:<12} {:<12} {:<20} COMMAND",
            "SESSION", "USER", "RUNAS", "DATE"
        );
        println!("{}", "-".repeat(76));

        for session in &sessions {
            let date = format_timestamp(session.timestamp);
            println!(
                "{:<12} {:<12} {:<12} {:<20} {}",
                session.id, session.user, session.target_user, date, session.command
            );
        }

        return 0;
    }

    // Replay mode.
    let session_id = match &opts.session_id {
        Some(id) => id.clone(),
        None => {
            eprintln!("sudoreplay: no session specified");
            print_sudoreplay_usage();
            return 1;
        }
    };

    match replay_session(&opts.directory, &session_id, opts.speed_factor) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("sudoreplay: {e}");
            1
        }
    }
}

// ============================================================================
// Main entry point
// ============================================================================

fn main() {
    let args: Vec<String> = env::args().collect();

    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("sudo");
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

    let personality = detect_personality(&prog_name);
    let rest = if args.len() > 1 { &args[1..] } else { &[] };

    let exit_code = match personality {
        Personality::Sudo => run_sudo(rest),
        Personality::Sudoedit => run_sudoedit(rest),
        Personality::Visudo => run_visudo(rest),
        Personality::Sudoreplay => run_sudoreplay(rest),
    };

    process::exit(exit_code);
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -- Personality detection tests --

    #[test]
    fn personality_detect_sudo() {
        assert_eq!(detect_personality("sudo"), Personality::Sudo);
    }

    #[test]
    fn personality_detect_sudo_with_path() {
        assert_eq!(detect_personality("/usr/bin/sudo"), Personality::Sudo);
    }

    #[test]
    fn personality_detect_sudo_windows_path() {
        assert_eq!(
            detect_personality("C:\\Windows\\sudo"),
            Personality::Sudo
        );
    }

    #[test]
    fn personality_detect_sudo_exe() {
        assert_eq!(detect_personality("sudo.exe"), Personality::Sudo);
    }

    #[test]
    fn personality_detect_sudoedit() {
        assert_eq!(detect_personality("sudoedit"), Personality::Sudoedit);
    }

    #[test]
    fn personality_detect_sudoedit_path() {
        assert_eq!(
            detect_personality("/usr/bin/sudoedit"),
            Personality::Sudoedit
        );
    }

    #[test]
    fn personality_detect_sudoedit_exe() {
        assert_eq!(
            detect_personality("sudoedit.exe"),
            Personality::Sudoedit
        );
    }

    #[test]
    fn personality_detect_visudo() {
        assert_eq!(detect_personality("visudo"), Personality::Visudo);
    }

    #[test]
    fn personality_detect_visudo_path() {
        assert_eq!(
            detect_personality("/usr/sbin/visudo"),
            Personality::Visudo
        );
    }

    #[test]
    fn personality_detect_visudo_exe() {
        assert_eq!(detect_personality("visudo.exe"), Personality::Visudo);
    }

    #[test]
    fn personality_detect_sudoreplay() {
        assert_eq!(
            detect_personality("sudoreplay"),
            Personality::Sudoreplay
        );
    }

    #[test]
    fn personality_detect_sudoreplay_path() {
        assert_eq!(
            detect_personality("/usr/bin/sudoreplay"),
            Personality::Sudoreplay
        );
    }

    #[test]
    fn personality_detect_unknown_defaults_sudo() {
        assert_eq!(detect_personality("foobar"), Personality::Sudo);
    }

    #[test]
    fn personality_detect_empty_defaults_sudo() {
        assert_eq!(detect_personality(""), Personality::Sudo);
    }

    #[test]
    fn personality_display() {
        assert_eq!(format!("{}", Personality::Sudo), "sudo");
        assert_eq!(format!("{}", Personality::Sudoedit), "sudoedit");
        assert_eq!(format!("{}", Personality::Visudo), "visudo");
        assert_eq!(format!("{}", Personality::Sudoreplay), "sudoreplay");
    }

    // -- Sudoers parser tests --

    #[test]
    fn parse_empty_sudoers() {
        let config = parse_sudoers("").unwrap();
        assert!(config.privileges.is_empty());
        assert!(config.user_aliases.is_empty());
    }

    #[test]
    fn parse_comments_only() {
        let config = parse_sudoers("# This is a comment\n# Another comment\n").unwrap();
        assert!(config.privileges.is_empty());
    }

    #[test]
    fn parse_user_alias() {
        let config = parse_sudoers("User_Alias ADMINS = alice, bob, charlie\n").unwrap();
        assert_eq!(
            config.user_aliases.get("ADMINS").unwrap(),
            &vec!["alice".to_string(), "bob".to_string(), "charlie".to_string()]
        );
    }

    #[test]
    fn parse_host_alias() {
        let config =
            parse_sudoers("Host_Alias SERVERS = web1, web2, db1\n").unwrap();
        assert_eq!(
            config.host_aliases.get("SERVERS").unwrap(),
            &vec!["web1".to_string(), "web2".to_string(), "db1".to_string()]
        );
    }

    #[test]
    fn parse_cmnd_alias() {
        let config = parse_sudoers(
            "Cmnd_Alias NETWORKING = /sbin/ifconfig, /sbin/route, /sbin/iptables\n",
        )
        .unwrap();
        let members = config.cmnd_aliases.get("NETWORKING").unwrap();
        assert_eq!(members.len(), 3);
        assert_eq!(members[0], "/sbin/ifconfig");
    }

    #[test]
    fn parse_runas_alias() {
        let config = parse_sudoers("Runas_Alias WEB = www-data, nginx\n").unwrap();
        assert_eq!(
            config.runas_aliases.get("WEB").unwrap(),
            &vec!["www-data".to_string(), "nginx".to_string()]
        );
    }

    #[test]
    fn parse_multiple_aliases_on_one_line() {
        let config =
            parse_sudoers("User_Alias ADMINS = alice, bob : DEVS = charlie, dave\n")
                .unwrap();
        assert_eq!(config.user_aliases.get("ADMINS").unwrap().len(), 2);
        assert_eq!(config.user_aliases.get("DEVS").unwrap().len(), 2);
    }

    #[test]
    fn parse_defaults_boolean() {
        let config = parse_sudoers("Defaults requiretty\n").unwrap();
        assert!(config.is_default_set("requiretty"));
    }

    #[test]
    fn parse_defaults_negated() {
        let config = parse_sudoers("Defaults !requiretty\n").unwrap();
        assert!(!config.is_default_set("requiretty"));
    }

    #[test]
    fn parse_defaults_key_value() {
        let config = parse_sudoers("Defaults timestamp_timeout=10\n").unwrap();
        assert_eq!(config.get_default("timestamp_timeout"), Some("10"));
    }

    #[test]
    fn parse_defaults_env_keep() {
        let config =
            parse_sudoers("Defaults env_keep=\"SSH_AUTH_SOCK DISPLAY\"\n").unwrap();
        let keep = config.env_keep_list();
        assert!(keep.contains(&"SSH_AUTH_SOCK".to_string()));
        assert!(keep.contains(&"DISPLAY".to_string()));
    }

    #[test]
    fn parse_defaults_scoped_user() {
        let config = parse_sudoers("Defaults:alice !requiretty\n").unwrap();
        assert_eq!(config.defaults.len(), 1);
        assert_eq!(config.defaults[0].scope, ":alice");
    }

    #[test]
    fn parse_simple_privilege() {
        let config = parse_sudoers("root ALL = (ALL) ALL\n").unwrap();
        assert_eq!(config.privileges.len(), 1);
        assert_eq!(config.privileges[0].users, vec!["root"]);
        assert_eq!(config.privileges[0].hosts, vec!["ALL"]);
        assert_eq!(config.privileges[0].runas.users, vec!["ALL"]);
        assert_eq!(config.privileges[0].commands.len(), 1);
        assert_eq!(config.privileges[0].commands[0].command, "ALL");
    }

    #[test]
    fn parse_privilege_nopasswd() {
        let config =
            parse_sudoers("alice ALL = (root) NOPASSWD: /usr/bin/apt\n").unwrap();
        assert_eq!(config.privileges[0].commands[0].nopasswd, true);
        assert_eq!(config.privileges[0].commands[0].command, "/usr/bin/apt");
    }

    #[test]
    fn parse_privilege_multiple_commands() {
        let config = parse_sudoers(
            "bob ALL = (root) /usr/bin/apt, /usr/bin/systemctl, /usr/bin/journalctl\n",
        )
        .unwrap();
        assert_eq!(config.privileges[0].commands.len(), 3);
    }

    #[test]
    fn parse_privilege_mixed_tags() {
        let config = parse_sudoers(
            "alice ALL = (root) NOPASSWD: /usr/bin/apt, PASSWD: /usr/bin/rm\n",
        )
        .unwrap();
        assert!(config.privileges[0].commands[0].nopasswd);
        assert!(!config.privileges[0].commands[1].nopasswd);
    }

    #[test]
    fn parse_privilege_group_user() {
        let config = parse_sudoers("%wheel ALL = (ALL) ALL\n").unwrap();
        assert_eq!(config.privileges[0].users, vec!["%wheel"]);
    }

    #[test]
    fn parse_privilege_runas_with_group() {
        let config =
            parse_sudoers("alice ALL = (bob : www-data) /usr/bin/service\n").unwrap();
        assert_eq!(config.privileges[0].runas.users, vec!["bob"]);
        assert_eq!(config.privileges[0].runas.groups, vec!["www-data"]);
    }

    #[test]
    fn parse_privilege_no_runas() {
        let config = parse_sudoers("alice ALL = /usr/bin/ls\n").unwrap();
        assert_eq!(config.privileges[0].runas.users, vec!["root"]);
    }

    #[test]
    fn parse_line_continuation() {
        let config = parse_sudoers(
            "User_Alias ADMINS = alice, \\\n    bob, charlie\n",
        )
        .unwrap();
        assert_eq!(config.user_aliases.get("ADMINS").unwrap().len(), 3);
    }

    #[test]
    fn parse_noexec_tag() {
        let config =
            parse_sudoers("alice ALL = (root) NOEXEC: /usr/bin/vi\n").unwrap();
        assert!(config.privileges[0].commands[0].noexec);
    }

    #[test]
    fn parse_setenv_tag() {
        let config =
            parse_sudoers("alice ALL = (root) SETENV: /usr/bin/env\n").unwrap();
        assert!(config.privileges[0].commands[0].setenv);
    }

    #[test]
    fn parse_alias_missing_eq_is_error() {
        let result = parse_sudoers("User_Alias ADMINS alice bob\n");
        assert!(result.is_err());
    }

    #[test]
    fn parse_alias_empty_name_is_error() {
        let result = parse_sudoers("User_Alias  = alice, bob\n");
        assert!(result.is_err());
    }

    #[test]
    fn parse_alias_lowercase_name_is_error() {
        let result = parse_sudoers("User_Alias admins = alice, bob\n");
        assert!(result.is_err());
    }

    #[test]
    fn parse_include_is_skipped() {
        let config = parse_sudoers("#include /etc/sudoers.d/local\n").unwrap();
        assert!(config.privileges.is_empty());
    }

    #[test]
    fn parse_at_include_is_skipped() {
        let config = parse_sudoers("@include /etc/sudoers.d/local\n").unwrap();
        assert!(config.privileges.is_empty());
    }

    #[test]
    fn parse_complex_sudoers() {
        let content = "\
# Sudoers file
User_Alias ADMINS = alice, bob
Host_Alias SERVERS = web1, db1
Cmnd_Alias SERVICES = /usr/bin/systemctl, /usr/bin/journalctl

Defaults env_reset
Defaults timestamp_timeout=15
Defaults:alice !requiretty

root ALL = (ALL:ALL) ALL
%wheel ALL = (ALL) ALL
ADMINS SERVERS = (root) NOPASSWD: SERVICES
alice ALL = (root) /usr/bin/apt, NOPASSWD: /usr/bin/ls
";
        let config = parse_sudoers(content).unwrap();
        assert_eq!(config.user_aliases.len(), 1);
        assert_eq!(config.host_aliases.len(), 1);
        assert_eq!(config.cmnd_aliases.len(), 1);
        assert_eq!(config.privileges.len(), 4);
        assert_eq!(config.defaults.len(), 3);
    }

    // -- Timestamp tests --

    #[test]
    fn timestamp_timeout_default() {
        let config = SudoersConfig::new();
        assert_eq!(config.timestamp_timeout(), DEFAULT_TIMEOUT);
    }

    #[test]
    fn timestamp_timeout_custom() {
        let config = parse_sudoers("Defaults timestamp_timeout=10\n").unwrap();
        assert_eq!(config.timestamp_timeout(), 600); // 10 minutes = 600 seconds
    }

    #[test]
    fn timestamp_timeout_negative_never_expires() {
        let config = parse_sudoers("Defaults timestamp_timeout=-1\n").unwrap();
        assert_eq!(config.timestamp_timeout(), u64::MAX);
    }

    #[test]
    fn timestamp_timeout_zero() {
        let config = parse_sudoers("Defaults timestamp_timeout=0\n").unwrap();
        assert_eq!(config.timestamp_timeout(), 0);
    }

    // -- Authorization tests --

    #[test]
    fn auth_root_all() {
        let config = parse_sudoers("root ALL = (ALL) ALL\n").unwrap();
        let result = check_authorization(
            &config, "root", "localhost", "root", "", "/usr/bin/ls",
            &["root".to_string()],
        );
        assert!(result.is_some());
    }

    #[test]
    fn auth_user_not_authorized() {
        let config = parse_sudoers("root ALL = (ALL) ALL\n").unwrap();
        let result = check_authorization(
            &config, "alice", "localhost", "root", "", "/usr/bin/ls",
            &["alice".to_string()],
        );
        assert!(result.is_none());
    }

    #[test]
    fn auth_user_specific_command() {
        let config = parse_sudoers("alice ALL = (root) /usr/bin/apt\n").unwrap();
        let result = check_authorization(
            &config, "alice", "localhost", "root", "", "/usr/bin/apt",
            &["alice".to_string()],
        );
        assert!(result.is_some());
    }

    #[test]
    fn auth_user_wrong_command() {
        let config = parse_sudoers("alice ALL = (root) /usr/bin/apt\n").unwrap();
        let result = check_authorization(
            &config, "alice", "localhost", "root", "", "/usr/bin/rm",
            &["alice".to_string()],
        );
        assert!(result.is_none());
    }

    #[test]
    fn auth_group_match() {
        let config = parse_sudoers("%wheel ALL = (ALL) ALL\n").unwrap();
        let result = check_authorization(
            &config,
            "alice",
            "localhost",
            "root",
            "",
            "/usr/bin/ls",
            &["alice".to_string(), "wheel".to_string()],
        );
        assert!(result.is_some());
    }

    #[test]
    fn auth_group_no_match() {
        let config = parse_sudoers("%wheel ALL = (ALL) ALL\n").unwrap();
        let result = check_authorization(
            &config,
            "alice",
            "localhost",
            "root",
            "",
            "/usr/bin/ls",
            &["alice".to_string(), "users".to_string()],
        );
        assert!(result.is_none());
    }

    #[test]
    fn auth_user_alias() {
        let config = parse_sudoers(
            "User_Alias ADMINS = alice, bob\nADMINS ALL = (ALL) ALL\n",
        )
        .unwrap();
        let result = check_authorization(
            &config, "alice", "localhost", "root", "", "/usr/bin/ls",
            &["alice".to_string()],
        );
        assert!(result.is_some());
    }

    #[test]
    fn auth_host_mismatch() {
        let config = parse_sudoers("alice web1 = (root) ALL\n").unwrap();
        let result = check_authorization(
            &config, "alice", "db1", "root", "", "/usr/bin/ls",
            &["alice".to_string()],
        );
        assert!(result.is_none());
    }

    #[test]
    fn auth_host_match() {
        let config = parse_sudoers("alice web1 = (root) ALL\n").unwrap();
        let result = check_authorization(
            &config, "alice", "web1", "root", "", "/usr/bin/ls",
            &["alice".to_string()],
        );
        assert!(result.is_some());
    }

    #[test]
    fn auth_host_alias() {
        let config = parse_sudoers(
            "Host_Alias SERVERS = web1, web2\nalice SERVERS = (root) ALL\n",
        )
        .unwrap();
        let result = check_authorization(
            &config, "alice", "web2", "root", "", "/usr/bin/ls",
            &["alice".to_string()],
        );
        assert!(result.is_some());
    }

    #[test]
    fn auth_runas_mismatch() {
        let config = parse_sudoers("alice ALL = (bob) ALL\n").unwrap();
        let result = check_authorization(
            &config, "alice", "localhost", "root", "", "/usr/bin/ls",
            &["alice".to_string()],
        );
        assert!(result.is_none());
    }

    #[test]
    fn auth_cmnd_alias() {
        let config = parse_sudoers(
            "Cmnd_Alias NET = /sbin/ifconfig, /sbin/route\nalice ALL = (root) NET\n",
        )
        .unwrap();
        let result = check_authorization(
            &config, "alice", "localhost", "root", "", "/sbin/ifconfig",
            &["alice".to_string()],
        );
        assert!(result.is_some());
    }

    #[test]
    fn auth_nopasswd_flag() {
        let config =
            parse_sudoers("alice ALL = (root) NOPASSWD: /usr/bin/apt\n").unwrap();
        let spec = check_authorization(
            &config, "alice", "localhost", "root", "", "/usr/bin/apt",
            &["alice".to_string()],
        );
        assert!(spec.is_some());
        assert!(spec.unwrap().nopasswd);
    }

    #[test]
    fn auth_last_match_wins() {
        let config = parse_sudoers(
            "alice ALL = (root) /usr/bin/ls\nalice ALL = (root) NOPASSWD: /usr/bin/ls\n",
        )
        .unwrap();
        let spec = check_authorization(
            &config, "alice", "localhost", "root", "", "/usr/bin/ls",
            &["alice".to_string()],
        );
        assert!(spec.is_some());
        assert!(spec.unwrap().nopasswd);
    }

    #[test]
    fn auth_wildcard_command() {
        let config = parse_sudoers("alice ALL = (root) /usr/bin/*\n").unwrap();
        let result = check_authorization(
            &config, "alice", "localhost", "root", "", "/usr/bin/anything",
            &["alice".to_string()],
        );
        assert!(result.is_some());
    }

    #[test]
    fn auth_wildcard_no_match_different_dir() {
        let config = parse_sudoers("alice ALL = (root) /usr/bin/*\n").unwrap();
        let result = check_authorization(
            &config, "alice", "localhost", "root", "", "/usr/sbin/something",
            &["alice".to_string()],
        );
        assert!(result.is_none());
    }

    // -- Command matching tests --

    #[test]
    fn command_match_exact() {
        let aliases = HashMap::new();
        assert!(command_matches("/usr/bin/ls", "", "/usr/bin/ls", &aliases));
    }

    #[test]
    fn command_match_all() {
        let aliases = HashMap::new();
        assert!(command_matches("ALL", "", "/any/command", &aliases));
    }

    #[test]
    fn command_match_wildcard() {
        let aliases = HashMap::new();
        assert!(command_matches("/usr/bin/*", "", "/usr/bin/ls", &aliases));
    }

    #[test]
    fn command_no_match() {
        let aliases = HashMap::new();
        assert!(!command_matches("/usr/bin/ls", "", "/usr/bin/rm", &aliases));
    }

    #[test]
    fn command_match_negation() {
        let aliases = HashMap::new();
        assert!(!command_matches("!/usr/bin/rm", "", "/usr/bin/rm", &aliases));
    }

    #[test]
    fn command_path_match_exact() {
        assert!(command_path_matches("/usr/bin/ls", "/usr/bin/ls"));
    }

    #[test]
    fn command_path_match_wildcard() {
        assert!(command_path_matches("/usr/bin/*", "/usr/bin/ls"));
        assert!(command_path_matches("/usr/bin/*", "/usr/bin/cat"));
    }

    #[test]
    fn command_path_no_match_wildcard() {
        assert!(!command_path_matches("/usr/bin/*", "/usr/sbin/ls"));
    }

    #[test]
    fn command_path_basename_match() {
        assert!(command_path_matches("ls", "/usr/bin/ls"));
    }

    // -- User matching tests --

    #[test]
    fn user_match_exact() {
        let aliases = HashMap::new();
        assert!(user_matches(
            &["alice".to_string()],
            "alice",
            &[],
            &aliases,
        ));
    }

    #[test]
    fn user_match_all() {
        let aliases = HashMap::new();
        assert!(user_matches(
            &["ALL".to_string()],
            "anyone",
            &[],
            &aliases,
        ));
    }

    #[test]
    fn user_match_group() {
        let aliases = HashMap::new();
        assert!(user_matches(
            &["%wheel".to_string()],
            "alice",
            &["wheel".to_string()],
            &aliases,
        ));
    }

    #[test]
    fn user_no_match() {
        let aliases = HashMap::new();
        assert!(!user_matches(
            &["bob".to_string()],
            "alice",
            &[],
            &aliases,
        ));
    }

    #[test]
    fn user_match_via_alias() {
        let mut aliases = HashMap::new();
        aliases.insert(
            "ADMINS".to_string(),
            vec!["alice".to_string(), "bob".to_string()],
        );
        assert!(user_matches(
            &["ADMINS".to_string()],
            "alice",
            &[],
            &aliases,
        ));
    }

    // -- Host matching tests --

    #[test]
    fn host_match_exact() {
        let aliases = HashMap::new();
        assert!(host_matches(
            &["web1".to_string()],
            "web1",
            &aliases,
        ));
    }

    #[test]
    fn host_match_all() {
        let aliases = HashMap::new();
        assert!(host_matches(
            &["ALL".to_string()],
            "anything",
            &aliases,
        ));
    }

    #[test]
    fn host_no_match() {
        let aliases = HashMap::new();
        assert!(!host_matches(
            &["web1".to_string()],
            "db1",
            &aliases,
        ));
    }

    #[test]
    fn host_match_via_alias() {
        let mut aliases = HashMap::new();
        aliases.insert(
            "SERVERS".to_string(),
            vec!["web1".to_string(), "web2".to_string()],
        );
        assert!(host_matches(
            &["SERVERS".to_string()],
            "web2",
            &aliases,
        ));
    }

    // -- Runas matching tests --

    #[test]
    fn runas_match_user() {
        let runas = RunasSpec {
            users: vec!["root".to_string()],
            groups: Vec::new(),
        };
        let aliases = HashMap::new();
        assert!(runas_matches(&runas, "root", "", &aliases));
    }

    #[test]
    fn runas_match_all() {
        let runas = RunasSpec {
            users: vec!["ALL".to_string()],
            groups: Vec::new(),
        };
        let aliases = HashMap::new();
        assert!(runas_matches(&runas, "anyone", "", &aliases));
    }

    #[test]
    fn runas_no_match() {
        let runas = RunasSpec {
            users: vec!["root".to_string()],
            groups: Vec::new(),
        };
        let aliases = HashMap::new();
        assert!(!runas_matches(&runas, "bob", "", &aliases));
    }

    #[test]
    fn runas_match_with_group() {
        let runas = RunasSpec {
            users: vec!["root".to_string()],
            groups: vec!["www-data".to_string()],
        };
        let aliases = HashMap::new();
        assert!(runas_matches(&runas, "root", "www-data", &aliases));
    }

    #[test]
    fn runas_group_mismatch() {
        let runas = RunasSpec {
            users: vec!["root".to_string()],
            groups: vec!["www-data".to_string()],
        };
        let aliases = HashMap::new();
        assert!(!runas_matches(&runas, "root", "staff", &aliases));
    }

    // -- Prompt expansion tests --

    #[test]
    fn prompt_expand_user() {
        assert_eq!(
            expand_prompt("[sudo] password for %u: ", "alice", "host", "root"),
            "[sudo] password for alice: "
        );
    }

    #[test]
    fn prompt_expand_target_user() {
        assert_eq!(
            expand_prompt("Password for %U: ", "alice", "host", "root"),
            "Password for root: "
        );
    }

    #[test]
    fn prompt_expand_hostname() {
        assert_eq!(
            expand_prompt("%h password: ", "alice", "myhost", "root"),
            "myhost password: "
        );
    }

    #[test]
    fn prompt_expand_percent() {
        assert_eq!(
            expand_prompt("100%% done for %u: ", "alice", "host", "root"),
            "100% done for alice: "
        );
    }

    #[test]
    fn prompt_expand_no_placeholders() {
        assert_eq!(
            expand_prompt("Enter password: ", "alice", "host", "root"),
            "Enter password: "
        );
    }

    #[test]
    fn prompt_expand_multiple() {
        assert_eq!(
            expand_prompt("%u@%h as %U: ", "alice", "myhost", "root"),
            "alice@myhost as root: "
        );
    }

    // -- Timestamp formatting tests --

    #[test]
    fn format_timestamp_epoch_zero() {
        assert_eq!(format_timestamp(0), "1970-01-01 00:00:00");
    }

    #[test]
    fn format_timestamp_known_date() {
        // 2024-01-01 00:00:00 UTC = 1704067200
        let ts = format_timestamp(1_704_067_200);
        assert_eq!(ts, "2024-01-01 00:00:00");
    }

    #[test]
    fn format_timestamp_with_time() {
        // 1970-01-01 01:30:45 = 5445 seconds
        let ts = format_timestamp(5445);
        assert_eq!(ts, "1970-01-01 01:30:45");
    }

    // -- Date calculation tests --

    #[test]
    fn leap_year_check() {
        assert!(is_leap_year(2000));
        assert!(is_leap_year(2024));
        assert!(!is_leap_year(1900));
        assert!(!is_leap_year(2023));
        assert!(is_leap_year(2400));
    }

    #[test]
    fn days_to_date_epoch() {
        assert_eq!(days_to_date(0), (1970, 1, 1));
    }

    #[test]
    fn days_to_date_end_of_jan() {
        assert_eq!(days_to_date(30), (1970, 1, 31));
    }

    #[test]
    fn days_to_date_feb_1() {
        assert_eq!(days_to_date(31), (1970, 2, 1));
    }

    #[test]
    fn days_to_date_year_boundary() {
        assert_eq!(days_to_date(365), (1971, 1, 1));
    }

    #[test]
    fn days_to_date_leap_year() {
        // 2000-03-01: days from epoch
        // 1970 to 2000 = 30 years: 7 leap years (72,76,80,84,88,92,96)
        // 30*365 + 7 + 31 + 29 = 10950 + 7 + 60 = 11017
        let (y, m, d) = days_to_date(11017);
        assert_eq!(y, 2000);
        assert_eq!(m, 3);
        assert_eq!(d, 1);
    }

    // -- JSON escape tests --

    #[test]
    fn json_escape_plain() {
        assert_eq!(json_escape("hello world"), "hello world");
    }

    #[test]
    fn json_escape_quotes() {
        assert_eq!(json_escape("he\"llo"), "he\\\"llo");
    }

    #[test]
    fn json_escape_backslash() {
        assert_eq!(json_escape("a\\b"), "a\\\\b");
    }

    #[test]
    fn json_escape_newline() {
        assert_eq!(json_escape("a\nb"), "a\\nb");
    }

    #[test]
    fn json_escape_tab() {
        assert_eq!(json_escape("a\tb"), "a\\tb");
    }

    #[test]
    fn json_escape_carriage_return() {
        assert_eq!(json_escape("a\rb"), "a\\rb");
    }

    #[test]
    fn json_escape_control_char() {
        let s = String::from("\x01");
        assert_eq!(json_escape(&s), "\\u0001");
    }

    // -- Environment handling tests --

    #[test]
    fn env_keep_includes_defaults() {
        let config = SudoersConfig::new();
        let keep = config.env_keep_list();
        assert!(keep.contains(&"TERM".to_string()));
        assert!(keep.contains(&"PATH".to_string()));
        assert!(keep.contains(&"HOME".to_string()));
    }

    #[test]
    fn env_keep_extended() {
        let config = parse_sudoers("Defaults env_keep=\"CUSTOM_VAR\"\n").unwrap();
        let keep = config.env_keep_list();
        assert!(keep.contains(&"CUSTOM_VAR".to_string()));
        assert!(keep.contains(&"TERM".to_string()));
    }

    #[test]
    fn env_check_empty_by_default() {
        let config = SudoersConfig::new();
        assert!(config.env_check_list().is_empty());
    }

    #[test]
    fn env_check_parsed() {
        let config =
            parse_sudoers("Defaults env_check=\"LD_LIBRARY_PATH\"\n").unwrap();
        let check = config.env_check_list();
        assert!(check.contains(&"LD_LIBRARY_PATH".to_string()));
    }

    // -- Sudo option parsing tests --

    #[test]
    fn parse_sudo_args_simple_command() {
        let args = vec!["ls".to_string(), "-la".to_string()];
        let opts = parse_sudo_args(&args).unwrap();
        assert_eq!(opts.command, vec!["ls", "-la"]);
        assert_eq!(opts.target_user, "root");
    }

    #[test]
    fn parse_sudo_args_target_user() {
        let args = vec!["-u".to_string(), "bob".to_string(), "ls".to_string()];
        let opts = parse_sudo_args(&args).unwrap();
        assert_eq!(opts.target_user, "bob");
        assert_eq!(opts.command, vec!["ls"]);
    }

    #[test]
    fn parse_sudo_args_target_group() {
        let args = vec![
            "-g".to_string(),
            "staff".to_string(),
            "ls".to_string(),
        ];
        let opts = parse_sudo_args(&args).unwrap();
        assert_eq!(opts.target_group, "staff");
    }

    #[test]
    fn parse_sudo_args_login_shell() {
        let args = vec!["-i".to_string()];
        let opts = parse_sudo_args(&args).unwrap();
        assert!(opts.login_shell);
        assert!(opts.command.is_empty());
    }

    #[test]
    fn parse_sudo_args_shell() {
        let args = vec!["-s".to_string(), "ls".to_string()];
        let opts = parse_sudo_args(&args).unwrap();
        assert!(opts.shell);
    }

    #[test]
    fn parse_sudo_args_list() {
        let args = vec!["-l".to_string()];
        let opts = parse_sudo_args(&args).unwrap();
        assert!(opts.list);
    }

    #[test]
    fn parse_sudo_args_validate() {
        let args = vec!["-v".to_string()];
        let opts = parse_sudo_args(&args).unwrap();
        assert!(opts.validate);
    }

    #[test]
    fn parse_sudo_args_invalidate() {
        let args = vec!["-k".to_string()];
        let opts = parse_sudo_args(&args).unwrap();
        assert!(opts.invalidate);
    }

    #[test]
    fn parse_sudo_args_remove_timestamp() {
        let args = vec!["-K".to_string()];
        let opts = parse_sudo_args(&args).unwrap();
        assert!(opts.remove_timestamp);
    }

    #[test]
    fn parse_sudo_args_non_interactive() {
        let args = vec!["-n".to_string(), "ls".to_string()];
        let opts = parse_sudo_args(&args).unwrap();
        assert!(opts.non_interactive);
    }

    #[test]
    fn parse_sudo_args_background() {
        let args = vec!["-b".to_string(), "sleep".to_string(), "60".to_string()];
        let opts = parse_sudo_args(&args).unwrap();
        assert!(opts.background);
    }

    #[test]
    fn parse_sudo_args_edit() {
        let args = vec!["-e".to_string(), "/etc/hosts".to_string()];
        let opts = parse_sudo_args(&args).unwrap();
        assert!(opts.edit_mode);
    }

    #[test]
    fn parse_sudo_args_preserve_env() {
        let args = vec!["-E".to_string(), "ls".to_string()];
        let opts = parse_sudo_args(&args).unwrap();
        assert!(opts.preserve_env);
    }

    #[test]
    fn parse_sudo_args_custom_prompt() {
        let args = vec![
            "-p".to_string(),
            "Enter: ".to_string(),
            "ls".to_string(),
        ];
        let opts = parse_sudo_args(&args).unwrap();
        assert_eq!(opts.prompt, "Enter: ");
    }

    #[test]
    fn parse_sudo_args_double_dash() {
        let args = vec![
            "-u".to_string(),
            "root".to_string(),
            "--".to_string(),
            "-k".to_string(),
        ];
        let opts = parse_sudo_args(&args).unwrap();
        assert_eq!(opts.command, vec!["-k"]);
        assert!(!opts.invalidate);
    }

    #[test]
    fn parse_sudo_args_combined_flags() {
        let args = vec!["-inE".to_string(), "ls".to_string()];
        let opts = parse_sudo_args(&args).unwrap();
        assert!(opts.login_shell);
        assert!(opts.non_interactive);
        assert!(opts.preserve_env);
    }

    #[test]
    fn parse_sudo_args_unknown_flag() {
        let args = vec!["-Z".to_string()];
        assert!(parse_sudo_args(&args).is_err());
    }

    #[test]
    fn parse_sudo_args_u_missing_value() {
        let args = vec!["-u".to_string()];
        assert!(parse_sudo_args(&args).is_err());
    }

    #[test]
    fn parse_sudo_args_empty() {
        let args: Vec<String> = vec![];
        let opts = parse_sudo_args(&args).unwrap();
        assert!(opts.command.is_empty());
        assert_eq!(opts.target_user, "root");
    }

    // -- Visudo option parsing tests --

    #[test]
    fn parse_visudo_defaults() {
        let args: Vec<String> = vec![];
        let opts = parse_visudo_args(&args).unwrap();
        assert!(!opts.check_only);
        assert_eq!(opts.file, SUDOERS_PATH);
        assert!(!opts.strict);
    }

    #[test]
    fn parse_visudo_check_only() {
        let args = vec!["-c".to_string()];
        let opts = parse_visudo_args(&args).unwrap();
        assert!(opts.check_only);
    }

    #[test]
    fn parse_visudo_alternate_file() {
        let args = vec!["-f".to_string(), "/tmp/sudoers".to_string()];
        let opts = parse_visudo_args(&args).unwrap();
        assert_eq!(opts.file, "/tmp/sudoers");
    }

    #[test]
    fn parse_visudo_strict() {
        let args = vec!["-s".to_string()];
        let opts = parse_visudo_args(&args).unwrap();
        assert!(opts.strict);
    }

    #[test]
    fn parse_visudo_unknown_flag() {
        let args = vec!["-z".to_string()];
        assert!(parse_visudo_args(&args).is_err());
    }

    #[test]
    fn parse_visudo_f_missing_value() {
        let args = vec!["-f".to_string()];
        assert!(parse_visudo_args(&args).is_err());
    }

    // -- Sudoreplay option parsing tests --

    #[test]
    fn parse_sudoreplay_defaults() {
        let args: Vec<String> = vec![];
        let opts = parse_sudoreplay_args(&args).unwrap();
        assert!(!opts.list);
        assert_eq!(opts.directory, SUDO_IO_DIR);
        assert!((opts.speed_factor - 1.0).abs() < f64::EPSILON);
        assert!(opts.session_id.is_none());
    }

    #[test]
    fn parse_sudoreplay_list() {
        let args = vec!["-l".to_string()];
        let opts = parse_sudoreplay_args(&args).unwrap();
        assert!(opts.list);
    }

    #[test]
    fn parse_sudoreplay_directory() {
        let args = vec!["-d".to_string(), "/tmp/logs".to_string()];
        let opts = parse_sudoreplay_args(&args).unwrap();
        assert_eq!(opts.directory, "/tmp/logs");
    }

    #[test]
    fn parse_sudoreplay_speed() {
        let args = vec!["-s".to_string(), "2.5".to_string()];
        let opts = parse_sudoreplay_args(&args).unwrap();
        assert!((opts.speed_factor - 2.5).abs() < f64::EPSILON);
    }

    #[test]
    fn parse_sudoreplay_session_id() {
        let args = vec!["abc123".to_string()];
        let opts = parse_sudoreplay_args(&args).unwrap();
        assert_eq!(opts.session_id.as_deref(), Some("abc123"));
    }

    #[test]
    fn parse_sudoreplay_negative_speed() {
        let args = vec!["-s".to_string(), "-1".to_string()];
        assert!(parse_sudoreplay_args(&args).is_err());
    }

    #[test]
    fn parse_sudoreplay_zero_speed() {
        let args = vec!["-s".to_string(), "0".to_string()];
        assert!(parse_sudoreplay_args(&args).is_err());
    }

    #[test]
    fn parse_sudoreplay_invalid_speed() {
        let args = vec!["-s".to_string(), "notanumber".to_string()];
        assert!(parse_sudoreplay_args(&args).is_err());
    }

    // -- Validation tests --

    #[test]
    fn validate_valid_sudoers() {
        let content = "root ALL = (ALL) ALL\n";
        let errors = validate_sudoers(content, false);
        assert!(errors.is_empty());
    }

    #[test]
    fn validate_invalid_alias_missing_eq() {
        let content = "User_Alias ADMINS alice bob\n";
        let errors = validate_sudoers(content, false);
        assert!(!errors.is_empty());
        assert!(!errors[0].is_warning);
    }

    #[test]
    fn validate_invalid_alias_empty_name() {
        let content = "User_Alias  = alice, bob\n";
        let errors = validate_sudoers(content, false);
        assert!(!errors.is_empty());
    }

    #[test]
    fn validate_invalid_alias_lowercase() {
        let content = "User_Alias admins = alice, bob\n";
        let errors = validate_sudoers(content, false);
        assert!(!errors.is_empty());
    }

    #[test]
    fn validate_missing_eq_in_priv() {
        let content = "alice ALL ALL\n";
        let errors = validate_sudoers(content, false);
        assert!(!errors.is_empty());
    }

    #[test]
    fn validate_unterminated_continuation() {
        let content = "User_Alias ADMINS = alice, \\\n";
        let errors = validate_sudoers(content, false);
        // Should report unterminated continuation.
        assert!(!errors.is_empty());
    }

    #[test]
    fn validate_comments_are_ok() {
        let content = "# This is fine\n# So is this\n";
        let errors = validate_sudoers(content, false);
        assert!(errors.is_empty());
    }

    #[test]
    fn validate_includes_are_ok() {
        let content = "#include /etc/sudoers.d/local\n@includedir /etc/sudoers.d\n";
        let errors = validate_sudoers(content, false);
        assert!(errors.is_empty());
    }

    // -- List privileges tests --

    #[test]
    fn list_privs_no_match() {
        let config = parse_sudoers("root ALL = (ALL) ALL\n").unwrap();
        let output = list_privileges(&config, "nobody", "localhost", &["nobody".to_string()]);
        assert!(output.contains("(none)"));
    }

    #[test]
    fn list_privs_with_match() {
        let config = parse_sudoers("alice ALL = (root) /usr/bin/ls\n").unwrap();
        let output = list_privileges(&config, "alice", "localhost", &["alice".to_string()]);
        assert!(output.contains("/usr/bin/ls"));
        assert!(output.contains("(root)"));
    }

    #[test]
    fn list_privs_nopasswd() {
        let config =
            parse_sudoers("alice ALL = (root) NOPASSWD: /usr/bin/apt\n").unwrap();
        let output = list_privileges(&config, "alice", "localhost", &["alice".to_string()]);
        assert!(output.contains("NOPASSWD:"));
    }

    // -- Error display tests --

    #[test]
    fn error_display_permission_denied() {
        let e = SudoError::_PermissionDenied("test".to_string());
        assert_eq!(format!("{e}"), "permission denied: test");
    }

    #[test]
    fn error_display_parse_error() {
        let e = SudoError::ParseError("bad syntax".to_string());
        assert_eq!(format!("{e}"), "parse error: bad syntax");
    }

    #[test]
    fn error_display_io_error() {
        let e = SudoError::IoError("file not found".to_string());
        assert_eq!(format!("{e}"), "I/O error: file not found");
    }

    #[test]
    fn error_display_invalid_config() {
        let e = SudoError::InvalidConfig("bad config".to_string());
        assert_eq!(format!("{e}"), "invalid configuration: bad config");
    }

    #[test]
    fn error_display_auth_error() {
        let e = SudoError::AuthError("wrong password".to_string());
        assert_eq!(format!("{e}"), "authentication error: wrong password");
    }

    #[test]
    fn error_display_usage_error() {
        let e = SudoError::UsageError("bad usage".to_string());
        assert_eq!(format!("{e}"), "usage error: bad usage");
    }

    #[test]
    fn error_display_timestamp_error() {
        let e = SudoError::TimestampError("expired".to_string());
        assert_eq!(format!("{e}"), "timestamp error: expired");
    }

    #[test]
    fn error_display_lock_error() {
        let e = SudoError::LockError("locked".to_string());
        assert_eq!(format!("{e}"), "lock error: locked");
    }

    #[test]
    fn error_from_io_error() {
        let io_err = io::Error::new(io::ErrorKind::NotFound, "not found");
        let sudo_err: SudoError = io_err.into();
        assert!(format!("{sudo_err}").contains("not found"));
    }

    // -- find_eq_outside_parens tests --

    #[test]
    fn find_eq_simple() {
        assert_eq!(find_eq_outside_parens("a = b"), Some(2));
    }

    #[test]
    fn find_eq_inside_parens() {
        assert_eq!(find_eq_outside_parens("a (x=y) = b"), Some(8));
    }

    #[test]
    fn find_eq_no_eq() {
        assert_eq!(find_eq_outside_parens("no equals here"), None);
    }

    #[test]
    fn find_eq_nested_parens() {
        assert_eq!(find_eq_outside_parens("a ((x=y)) = b"), Some(10));
    }

    // -- Format runas tests --

    #[test]
    fn format_runas_user_only() {
        let runas = RunasSpec {
            users: vec!["root".to_string()],
            groups: Vec::new(),
        };
        assert_eq!(format_runas(&runas), "root");
    }

    #[test]
    fn format_runas_user_and_group() {
        let runas = RunasSpec {
            users: vec!["root".to_string()],
            groups: vec!["www-data".to_string()],
        };
        assert_eq!(format_runas(&runas), "root : www-data");
    }

    #[test]
    fn format_runas_multiple_users() {
        let runas = RunasSpec {
            users: vec!["root".to_string(), "bob".to_string()],
            groups: Vec::new(),
        };
        assert_eq!(format_runas(&runas), "root, bob");
    }

    // -- Format tags tests --

    #[test]
    fn format_tags_nopasswd() {
        let cmnd = CmndSpec {
            nopasswd: true,
            noexec: false,
            setenv: false,
            command: "ALL".to_string(),
            args: String::new(),
        };
        assert_eq!(format_tags(&cmnd), "NOPASSWD: ");
    }

    #[test]
    fn format_tags_multiple() {
        let cmnd = CmndSpec {
            nopasswd: true,
            noexec: true,
            setenv: true,
            command: "ALL".to_string(),
            args: String::new(),
        };
        assert_eq!(format_tags(&cmnd), "NOPASSWD: NOEXEC: SETENV: ");
    }

    #[test]
    fn format_tags_none() {
        let cmnd = CmndSpec {
            nopasswd: false,
            noexec: false,
            setenv: false,
            command: "ALL".to_string(),
            args: String::new(),
        };
        assert_eq!(format_tags(&cmnd), "");
    }

    // -- Syntax error display test --

    #[test]
    fn syntax_error_display() {
        let err = SyntaxError {
            line_num: 5,
            message: "missing '='".to_string(),
            is_warning: false,
        };
        assert_eq!(format!("{err}"), "line 5: error: missing '='");
    }

    #[test]
    fn syntax_warning_display() {
        let err = SyntaxError {
            line_num: 10,
            message: "empty directive".to_string(),
            is_warning: true,
        };
        assert_eq!(format!("{err}"), "line 10: warning: empty directive");
    }

    // -- Parse runas prefix tests --

    #[test]
    fn parse_runas_no_parens() {
        let (runas, rest) = parse_runas_prefix("/usr/bin/ls");
        assert_eq!(runas.users, vec!["root"]);
        assert_eq!(rest, "/usr/bin/ls");
    }

    #[test]
    fn parse_runas_user_only() {
        let (runas, rest) = parse_runas_prefix("(bob) /usr/bin/ls");
        assert_eq!(runas.users, vec!["bob"]);
        assert!(runas.groups.is_empty());
        assert_eq!(rest, "/usr/bin/ls");
    }

    #[test]
    fn parse_runas_user_and_group() {
        let (runas, rest) = parse_runas_prefix("(bob : staff) /usr/bin/ls");
        assert_eq!(runas.users, vec!["bob"]);
        assert_eq!(runas.groups, vec!["staff"]);
        assert_eq!(rest, "/usr/bin/ls");
    }

    #[test]
    fn parse_runas_all() {
        let (runas, _rest) = parse_runas_prefix("(ALL : ALL) ALL");
        assert_eq!(runas.users, vec!["ALL"]);
        assert_eq!(runas.groups, vec!["ALL"]);
    }

    #[test]
    fn parse_runas_empty_users_defaults_root() {
        let (runas, _) = parse_runas_prefix("( : staff) /bin/ls");
        assert_eq!(runas.users, vec!["root"]);
        assert_eq!(runas.groups, vec!["staff"]);
    }

    // -- set_or_replace tests --

    #[test]
    fn set_or_replace_new() {
        let mut env: Vec<(String, String)> = vec![];
        set_or_replace(&mut env, "KEY", "val");
        assert_eq!(env.len(), 1);
        assert_eq!(env[0], ("KEY".to_string(), "val".to_string()));
    }

    #[test]
    fn set_or_replace_existing() {
        let mut env = vec![("KEY".to_string(), "old".to_string())];
        set_or_replace(&mut env, "KEY", "new");
        assert_eq!(env.len(), 1);
        assert_eq!(env[0].1, "new");
    }

    // -- Defaults parsing edge cases --

    #[test]
    fn defaults_multiple_settings() {
        let config = parse_sudoers("Defaults env_reset, requiretty\n").unwrap();
        assert!(config.is_default_set("env_reset"));
        assert!(config.is_default_set("requiretty"));
    }

    #[test]
    fn defaults_env_keep_append() {
        let config = parse_sudoers("Defaults env_keep+=\"MY_VAR\"\n").unwrap();
        let keep = config.env_keep_list();
        assert!(keep.contains(&"MY_VAR".to_string()));
    }

    // -- RunasSpec default --

    #[test]
    fn runas_spec_default() {
        let runas = RunasSpec::default();
        assert_eq!(runas.users, vec!["root".to_string()]);
        assert!(runas.groups.is_empty());
    }

    // -- SudoOpts default --

    #[test]
    fn sudo_opts_default() {
        let opts = SudoOpts::default();
        assert_eq!(opts.target_user, "root");
        assert!(opts.target_group.is_empty());
        assert!(!opts.login_shell);
        assert!(!opts.shell);
        assert!(!opts.list);
        assert!(!opts.validate);
        assert!(!opts.invalidate);
        assert!(!opts.remove_timestamp);
        assert!(!opts.non_interactive);
        assert!(!opts.background);
        assert!(!opts.edit_mode);
        assert!(!opts.preserve_env);
        assert_eq!(opts.prompt, DEFAULT_PROMPT);
        assert!(opts.command.is_empty());
    }

    // -- Combined flag parsing with value --

    #[test]
    fn parse_sudo_combined_with_user() {
        let args = vec!["-iubob".to_string(), "ls".to_string()];
        let opts = parse_sudo_args(&args).unwrap();
        assert!(opts.login_shell);
        assert_eq!(opts.target_user, "bob");
        assert_eq!(opts.command, vec!["ls"]);
    }

    #[test]
    fn parse_sudo_combined_flags_with_group() {
        let args = vec!["-gstaff".to_string(), "ls".to_string()];
        let opts = parse_sudo_args(&args).unwrap();
        assert_eq!(opts.target_group, "staff");
    }

    #[test]
    fn parse_sudo_combined_flags_with_prompt() {
        let args = vec!["-pEnter:".to_string(), "ls".to_string()];
        let opts = parse_sudo_args(&args).unwrap();
        assert_eq!(opts.prompt, "Enter:");
    }
}
