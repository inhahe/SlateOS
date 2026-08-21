//! Slate OS Privileged Command Execution Utility
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
    "TERM",
    "PATH",
    "HOME",
    "SHELL",
    "LOGNAME",
    "USER",
    "DISPLAY",
    "XAUTHORITY",
    "LANG",
    "LC_ALL",
    "LC_COLLATE",
    "LC_CTYPE",
    "LC_MESSAGES",
    "LC_MONETARY",
    "LC_NUMERIC",
    "LC_TIME",
    "TZ",
];

/// Environment variables that are always removed for security.
const ENV_BLACKLIST: &[&str] = &[
    "LD_PRELOAD",
    "LD_LIBRARY_PATH",
    "LD_AUDIT",
    "LD_BIND_NOW",
    "LD_DEBUG",
    "LD_DYNAMIC_WEAK",
    "LD_ORIGIN_PATH",
    "LD_PROFILE",
    "LD_SHOW_AUXV",
    "LD_USE_LOAD_BIAS",
    "LOCALDOMAIN",
    "RES_OPTIONS",
    "HOSTALIASES",
    "NLSPATH",
    "PATH_LOCALE",
    "TERMINFO",
    "TERMINFO_DIRS",
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
    // `rsplit` over both separators rather than a hand-rolled scan producing a
    // byte index to slice at: that index landed on a character boundary only
    // because `/` and `\` happen to be ASCII, which is a fact about the
    // separators rather than anything the code established. `rsplit` always
    // yields at least one item, so the fallback is unreachable.
    let base = argv0.rsplit(['/', '\\']).next().unwrap_or(argv0);
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

/// What shape a `Defaults` setting may legally take.
///
/// The shape is what makes a misspelling detectable. `Defaults timestamp_timout=5`
/// is not distinguishable from a valid line by looking at the line alone — only
/// by knowing that no setting is spelled that way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DefaultShape {
    /// A boolean: `name` sets it, `!name` clears it. Never carries a value.
    Flag,
    /// Carries exactly one value: `name=value`. `+=` and `-=` are meaningless.
    Value,
    /// A whitespace-separated list: `name=v` replaces, `name+=v` adds,
    /// `name-=v` removes.
    List,
}

/// How a `Defaults` setting was written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DefaultOp {
    /// `name` (a flag) or `name=value`.
    Set,
    /// `!name`.
    Negate,
    /// `name+=value`.
    Add,
    /// `name-=value`.
    Remove,
}

/// One setting within a `Defaults` directive.
///
/// The operator is kept rather than folded into the name at parse time. It used
/// to be folded in by accident: the name was taken as everything before the
/// first `=`, so `env_keep += "X"` was stored under the name `env_keep +`. Two
/// consumers compensated by matching three spellings each (`env_keep`,
/// `env_keep+=`, `env_keep+`) and every other consumer — `get_default`, and so
/// `timestamp_timeout` and `env_reset` — simply never saw a `+=` line at all.
/// That is the band-aid shape: a defect in one place paid for in several.
#[derive(Debug, Clone)]
struct DefaultSetting {
    /// The setting name, with no operator attached.
    name: String,
    /// How it was written.
    op: DefaultOp,
    /// The value; empty for `Flag` settings, whose truth is carried by `op`.
    value: String,
}

/// A Defaults directive from the sudoers file.
#[derive(Debug, Clone)]
struct DefaultsDirective {
    /// The scope (empty = global, "user:" prefix, "host:" prefix, etc.)
    scope: String,
    /// The settings on this line, in written order.
    settings: Vec<DefaultSetting>,
}

/// The `Defaults` settings whose *shape* this implementation knows.
///
/// **This list is knowingly incomplete.** Real sudo's catalogue is larger and
/// grows; a name missing from here is not evidence the name is wrong. That is
/// exactly why an unlisted name is reported as a *warning* and a listed name
/// used with the wrong operator is reported as an *error*: the second is a fact
/// about the grammar, the first is a fact about this table. A `visudo` that
/// refused to save a correct file because our table was short would be worse
/// than the silence it replaced — the administrator could not fix it.
static KNOWN_DEFAULTS: &[(&str, DefaultShape)] = &[
    // Flags.
    ("always_set_home", DefaultShape::Flag),
    ("authenticate", DefaultShape::Flag),
    ("env_editor", DefaultShape::Flag),
    ("env_reset", DefaultShape::Flag),
    ("fqdn", DefaultShape::Flag),
    ("ignore_dot", DefaultShape::Flag),
    ("insults", DefaultShape::Flag),
    ("log_input", DefaultShape::Flag),
    ("log_output", DefaultShape::Flag),
    ("mail_always", DefaultShape::Flag),
    ("mail_badpass", DefaultShape::Flag),
    ("mail_no_host", DefaultShape::Flag),
    ("mail_no_perms", DefaultShape::Flag),
    ("mail_no_user", DefaultShape::Flag),
    ("noexec", DefaultShape::Flag),
    ("path_info", DefaultShape::Flag),
    ("preserve_groups", DefaultShape::Flag),
    ("pwfeedback", DefaultShape::Flag),
    ("requiretty", DefaultShape::Flag),
    ("root_sudo", DefaultShape::Flag),
    ("rootpw", DefaultShape::Flag),
    ("runaspw", DefaultShape::Flag),
    ("set_home", DefaultShape::Flag),
    ("set_logname", DefaultShape::Flag),
    ("shell_noargs", DefaultShape::Flag),
    ("stay_setuid", DefaultShape::Flag),
    ("targetpw", DefaultShape::Flag),
    ("tty_tickets", DefaultShape::Flag),
    ("umask_override", DefaultShape::Flag),
    ("use_pty", DefaultShape::Flag),
    ("visiblepw", DefaultShape::Flag),
    // Single-valued settings.
    ("badpass_message", DefaultShape::Value),
    ("editor", DefaultShape::Value),
    ("iolog_dir", DefaultShape::Value),
    ("iolog_file", DefaultShape::Value),
    ("lecture", DefaultShape::Value),
    ("lecture_file", DefaultShape::Value),
    ("logfile", DefaultShape::Value),
    ("loglinelen", DefaultShape::Value),
    ("mailerpath", DefaultShape::Value),
    ("mailfrom", DefaultShape::Value),
    ("mailsub", DefaultShape::Value),
    ("mailto", DefaultShape::Value),
    ("passprompt", DefaultShape::Value),
    ("passwd_timeout", DefaultShape::Value),
    ("passwd_tries", DefaultShape::Value),
    ("runas_default", DefaultShape::Value),
    ("secure_path", DefaultShape::Value),
    ("syslog", DefaultShape::Value),
    ("timestamp_timeout", DefaultShape::Value),
    ("timestampdir", DefaultShape::Value),
    ("timestampowner", DefaultShape::Value),
    ("umask", DefaultShape::Value),
    ("verifypw", DefaultShape::Value),
    // Lists.
    ("env_check", DefaultShape::List),
    ("env_delete", DefaultShape::List),
    ("env_file", DefaultShape::List),
    ("env_keep", DefaultShape::List),
];

/// The settings this implementation actually acts on.
///
/// A name in [`KNOWN_DEFAULTS`] but not here parses cleanly and then does
/// nothing, which is the same silence the shape checks exist to break — so
/// `visudo` says so rather than letting the administrator believe
/// `Defaults requiretty` had an effect. Every entry added here must have a
/// consumer; the test `honoured_defaults_are_all_known` keeps the two lists
/// from drifting apart, which is the failure this tree keeps rediscovering
/// whenever two hand-maintained lists have to agree.
static HONOURED_DEFAULTS: &[&str] = &["env_check", "env_keep", "env_reset", "timestamp_timeout"];

/// Look up a setting's shape, or `None` if the name is not in [`KNOWN_DEFAULTS`].
fn default_shape(name: &str) -> Option<DefaultShape> {
    KNOWN_DEFAULTS
        .iter()
        .find(|(known, _)| *known == name)
        .map(|(_, shape)| *shape)
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

    /// Every globally-scoped setting named `key`, in written order.
    ///
    /// `'k` is separate from `'a` on purpose: tying the key's lifetime to the
    /// config's would make everything borrowed from the config live only as
    /// long as the *name that was looked up*, so `get_default` could not hand
    /// its result back to a caller holding only the config.
    fn global_settings<'a, 'k>(
        &'a self,
        key: &'k str,
    ) -> impl Iterator<Item = &'a DefaultSetting> + use<'a, 'k> {
        self.defaults
            .iter()
            .filter(|d| d.scope.is_empty())
            .flat_map(|d| d.settings.iter())
            .filter(move |s| s.name == key)
    }

    /// Get the value of a Defaults setting (global scope).
    ///
    /// Later lines win, as in sudo — the last `Defaults` mentioning a setting is
    /// the one in force. The old implementation returned the *first* match,
    /// so a file that overrode a setting further down kept the earlier value.
    fn get_default(&self, key: &str) -> Option<&str> {
        self.global_settings(key).last().map(|s| match s.op {
            // A flag's truth is in the operator, not the value; render it so
            // `is_default_set` and the `timestamp_timeout` parse both see a
            // string, as they did when everything was a string pair.
            DefaultOp::Negate => "false",
            _ if s.value.is_empty() => "true",
            _ => s.value.as_str(),
        })
    }

    /// Check if a Defaults flag is set (boolean setting).
    fn is_default_set(&self, key: &str) -> bool {
        self.get_default(key)
            .is_some_and(|v| v != "false" && v != "0")
    }

    /// Apply the `=`/`+=`/`-=` sequence for a list setting onto `base`.
    ///
    /// `=` replaces the accumulated list, `+=` appends, `-=` removes — the
    /// operators exist to be applied in order, which is why the parser keeps
    /// them instead of gluing them onto the name.
    fn resolve_list(&self, key: &str, base: &[&str]) -> Vec<String> {
        let mut result: Vec<String> = base.iter().map(|s| (*s).to_string()).collect();
        for setting in self.global_settings(key) {
            let words: Vec<&str> = setting
                .value
                .split_whitespace()
                .map(|w| w.trim_matches('"'))
                .filter(|w| !w.is_empty())
                .collect();
            match setting.op {
                DefaultOp::Set => result = words.iter().map(|w| (*w).to_string()).collect(),
                DefaultOp::Add => {
                    for word in words {
                        if !result.iter().any(|r| r == word) {
                            result.push(word.to_string());
                        }
                    }
                }
                DefaultOp::Remove => result.retain(|r| !words.iter().any(|w| r == w)),
                // `!env_keep` — sudoers' disable operator for a list.
                DefaultOp::Negate => result.clear(),
            }
        }
        result
    }

    /// Get env_keep list from Defaults.
    ///
    /// The built-in list is the *base* a bare `env_keep=` replaces, matching
    /// sudo: `Defaults env_keep = "X"` keeps only `X`, while
    /// `Defaults env_keep += "X"` keeps the built-ins and `X`. The old code
    /// could not tell those apart — it never saw the `+=` form at all — so it
    /// treated both as "add", and a file that deliberately narrowed the kept
    /// environment did not narrow it.
    fn env_keep_list(&self) -> Vec<String> {
        self.resolve_list("env_keep", DEFAULT_ENV_KEEP)
    }

    /// Get env_check list from Defaults.
    fn env_check_list(&self) -> Vec<String> {
        self.resolve_list("env_check", &[])
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
    if let Some(rest) = strip_defaults_keyword(line) {
        parse_defaults(rest, config)?;
        return Ok(());
    }

    // #include / #includedir (legacy format — also @include / @includedir).
    if line.starts_with("#include")
        || line.starts_with("@include")
        || line.starts_with("#includedir")
        || line.starts_with("@includedir")
    {
        // In Slate OS, includes are handled at a higher level; skip in parsing.
        return Ok(());
    }

    // Otherwise it is a user privilege specification.
    parse_privilege_spec(line, config)?;
    Ok(())
}

/// Parse an alias definition: `NAME = member1, member2, ...`
fn parse_alias(text: &str, aliases: &mut HashMap<String, Vec<String>>) -> Result<(), SudoError> {
    // Multiple aliases can be on one line, separated by `:`.
    for alias_part in text.split(':') {
        let alias_part = alias_part.trim();
        // `split_once` rather than `find` plus two slices: it hands back both
        // sides already past the delimiter, so nothing here depends on `=`
        // being one byte wide, and there is no index to get wrong.
        let (name, members_str) = alias_part
            .split_once('=')
            .ok_or_else(|| SudoError::ParseError(format!("missing '=' in alias: {alias_part}")))?;
        let name = name.trim().to_string();
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

/// Strip the `Defaults` keyword, but only where it really is the keyword.
///
/// A bare `strip_prefix("Defaults")` also fires on a line whose first word
/// merely begins with it — a user named `Defaultsfoo` — and the remainder is
/// then read as a settings list. That was harmless while no directive was ever
/// rejected; now that malformed ones are errors, it would make `visudo` refuse
/// a file that is entirely correct, which is the one failure a validator must
/// not have. The keyword ends at whitespace, at a scope sigil, or at the end of
/// the line.
fn strip_defaults_keyword(line: &str) -> Option<&str> {
    let rest = line.strip_prefix("Defaults")?;
    match rest.chars().next() {
        None => Some(rest),
        Some(c) if c.is_whitespace() || matches!(c, ':' | '@' | '!' | '>') => Some(rest),
        Some(_) => None,
    }
}

/// Parse a Defaults directive.
///
/// Rejects what it can *prove* is wrong — an empty setting name, a name with a
/// space in it, an unbalanced quote, a negated setting that also carries a
/// value, a scope with nothing scoped to it, and a known setting used with an
/// operator its shape forbids. It deliberately does **not** reject a setting
/// name merely because [`KNOWN_DEFAULTS`] has not heard of it; see that table's
/// note. `validate_sudoers_line` turns unknown names into warnings, which is
/// where an incomplete table can be reported without being able to block a save.
fn parse_defaults(rest: &str, config: &mut SudoersConfig) -> Result<(), SudoError> {
    // Determine scope: Defaults, Defaults:user, Defaults@host, Defaults!cmnd,
    // Defaults>runas.
    //
    // The sigil counts as a scope only when it is attached to the keyword with
    // no space, which is sudo's rule and is the only thing separating
    // `Defaults!/usr/bin/foo bar` (a command-scoped default) from
    // `Defaults !requiretty` (a negated global flag). `rest` therefore must be
    // examined before it is trimmed -- trimming first loses the distinction,
    // and the whole space of negated global flags is then read as scopes.
    let first = rest.chars().next();
    let (scope, settings_str) = if first.is_some_and(|c| matches!(c, ':' | '@' | '!' | '>')) {
        // `split_at` on the first char's own length rather than `[..1]`: the
        // sigils are ASCII, but that is a fact about the sigils and not
        // something the slice established.
        let (scope_char, after) = rest.split_at(first.map_or(0, char::len_utf8));
        let Some(space_pos) = after.find(char::is_whitespace) else {
            // A scope and nothing scoped to it. This used to return `Ok(())`,
            // discarding the line in silence — so `Defaults:alice` on its own
            // was accepted, did nothing, and looked to its author like it had
            // restricted something for alice.
            return Err(SudoError::ParseError(format!(
                "Defaults{rest}: scope with no settings after it"
            )));
        };
        let (scope_name, settings) = after.split_at(space_pos);
        if scope_name.trim().is_empty() {
            return Err(SudoError::ParseError(
                "empty scope in Defaults directive".to_string(),
            ));
        }
        (
            format!("{scope_char}{}", scope_name.trim()),
            settings.trim(),
        )
    } else {
        // Global defaults. Trimmed only here, after the sigil test above has
        // had its look at the unmodified string.
        let rest = rest.trim();
        (String::new(), rest)
    };

    if settings_str.is_empty() {
        return Err(SudoError::ParseError(
            "Defaults directive with no settings".to_string(),
        ));
    }

    let mut settings = Vec::new();
    for part in settings_str.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        settings.push(parse_default_setting(part)?);
    }

    if settings.is_empty() {
        return Err(SudoError::ParseError(
            "Defaults directive with no settings".to_string(),
        ));
    }

    config.defaults.push(DefaultsDirective { scope, settings });
    Ok(())
}

/// Parse one `name` / `!name` / `name=v` / `name+=v` / `name-=v` setting.
fn parse_default_setting(part: &str) -> Result<DefaultSetting, SudoError> {
    // The operator lives at the *first* `=`, and a `+` or `-` immediately
    // before that `=` is part of the operator rather than of the name.
    //
    // Scanning the whole string for `+=` or `-=` instead would be wrong twice
    // over: it would find one inside a quoted value (`passprompt="a-=b"` would
    // be read as the setting `passprompt="a` removing `b`), and splitting at the
    // first `=` and calling everything before it the name -- which is what this
    // used to do -- produced the name `env_keep +`.
    let (name_raw, op, value_raw) = match part.split_once('=') {
        Some((lhs, rhs)) => match lhs.trim() {
            trimmed if trimmed.ends_with('+') => (
                trimmed.strip_suffix('+').unwrap_or(trimmed),
                DefaultOp::Add,
                Some(rhs),
            ),
            trimmed if trimmed.ends_with('-') => (
                trimmed.strip_suffix('-').unwrap_or(trimmed),
                DefaultOp::Remove,
                Some(rhs),
            ),
            trimmed => (trimmed, DefaultOp::Set, Some(rhs)),
        },
        None => match part.strip_prefix('!') {
            Some(stripped) => (stripped, DefaultOp::Negate, None),
            None => (part, DefaultOp::Set, None),
        },
    };

    let name = name_raw.trim();

    // A leading `!` that survived the split is one of two mistakes, neither of
    // which can be a typo for anything valid: `!name=value` asserts two
    // contradictory things about one setting, and `!!name` repeats the
    // operator. Both are errors rather than a choice between the halves.
    if let Some(inner) = name.strip_prefix('!') {
        return Err(SudoError::ParseError(if op == DefaultOp::Negate {
            format!("repeated '!' in Defaults setting name: {part}")
        } else {
            format!(
                "Defaults setting '{}' is both negated and given a value",
                inner.trim()
            )
        }));
    }

    if name.is_empty() {
        return Err(SudoError::ParseError(format!(
            "empty setting name in Defaults: {part}"
        )));
    }
    // A space inside the name means the line was written as `passwd tries=3` or
    // a comma was forgotten between two settings. Either way the name cannot
    // match anything, so storing it would be storing a line that does nothing.
    if name.contains(char::is_whitespace) {
        return Err(SudoError::ParseError(format!(
            "Defaults setting name contains whitespace (missing comma?): {name}"
        )));
    }

    let value = match value_raw {
        None => String::new(),
        Some(raw) => {
            let raw = raw.trim();
            // An odd number of quotes means the value ran off the end of the
            // line. `trim_matches('"')` used to swallow that: `env_keep = "A B`
            // became the value `A B` and the file looked fine.
            if raw.matches('"').count() % 2 != 0 {
                return Err(SudoError::ParseError(format!(
                    "unterminated quote in Defaults value for '{name}'"
                )));
            }
            raw.trim_matches('"').to_string()
        }
    };

    // Shape checks run only for names we actually know the shape of. For an
    // unknown name there is no ground truth to check against, and inventing one
    // would reject correct files.
    if let Some(shape) = default_shape(name) {
        let bad = match (shape, op) {
            (DefaultShape::Flag, DefaultOp::Set) if value_raw.is_some() => {
                Some("is a boolean flag and takes no value")
            }
            (DefaultShape::Flag, DefaultOp::Add | DefaultOp::Remove) => {
                Some("is a boolean flag; '+=' and '-=' do not apply to it")
            }
            // `!env_keep` is legal and empties the list — sudoers documents `!`
            // as the "disable" operator for list settings alongside `=`/`+=`/`-=`.
            // A single-valued setting has nothing to disable, so `!secure_path`
            // stays an error.
            (DefaultShape::Value, DefaultOp::Negate) => {
                Some("takes a value and cannot be negated with '!'")
            }
            (DefaultShape::Value | DefaultShape::List, DefaultOp::Set) if value_raw.is_none() => {
                Some("requires a value, as in 'name=value'")
            }
            (DefaultShape::Value, DefaultOp::Add | DefaultOp::Remove) => {
                Some("holds a single value; '+=' and '-=' apply only to lists")
            }
            _ => None,
        };
        if let Some(reason) = bad {
            return Err(SudoError::ParseError(format!(
                "Defaults setting '{name}' {reason}"
            )));
        }
    }

    Ok(DefaultSetting {
        name: name.to_string(),
        op,
        value,
    })
}

/// Parse a user privilege specification line.
///
/// Format: `user host = (runas) NOPASSWD: command, command, ...`
fn parse_privilege_spec(line: &str, config: &mut SudoersConfig) -> Result<(), SudoError> {
    // Split at first `=` that is not inside parentheses.
    let (left, right) = split_at_eq_outside_parens(line).ok_or_else(|| {
        SudoError::ParseError(format!("missing '=' in privilege specification: {line}"))
    })?;
    let (left, right) = (left.trim(), right.trim());

    // Left side: user(s) host(s) separated by whitespace.
    // The last whitespace-separated token(s) before `=` are the hosts.
    // Simple heuristic: split by whitespace, first token is user spec,
    // remaining are hosts. If there is only one token, host is ALL.
    let left_parts: Vec<&str> = left.split_whitespace().collect();
    let Some((user_str, host_parts)) = left_parts.split_first() else {
        return Err(SudoError::ParseError(
            "empty left side of privilege spec".to_string(),
        ));
    };
    let (user_strs, host_strs) = if host_parts.is_empty() {
        (vec![*user_str], vec!["ALL"])
    } else {
        (vec![*user_str], host_parts.to_vec())
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

/// Split at the `=` that is not inside parentheses, returning both sides.
///
/// Returns the halves rather than the position, because the position was
/// only ever useful for producing them and made every caller re-derive the
/// `+ 1` that steps over the `=`. That step is correct here only because `=`
/// is one byte; expressed as `strip_prefix` it is correct because it strips
/// the character it names.
fn split_at_eq_outside_parens(s: &str) -> Option<(&str, &str)> {
    let mut depth = 0u32;
    for (i, c) in s.char_indices() {
        match c {
            '(' => depth = depth.saturating_add(1),
            ')' => depth = depth.saturating_sub(1),
            '=' if depth == 0 => {
                let (left, from_eq) = s.split_at(i);
                return Some((left, from_eq.strip_prefix('=').unwrap_or(from_eq)));
            }
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

    // `(` is known present from the `starts_with` above, and `split_once` takes
    // the rest apart at `)` without an index that has to step over it.
    if let Some(after_open) = trimmed.strip_prefix('(')
        && let Some((inner, rest)) = after_open.split_once(')')
    {
        let rest = rest.trim();
        let (user_part, group_part) = inner.split_once(':').unwrap_or((inner, ""));

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

/// The tags a command list may carry, and what each one sets.
///
/// One table rather than a chain of `strip_prefix` arms, so that "is this a
/// tag?" and "what does it do?" cannot disagree — the check that rejects an
/// unknown tag below reads the same list the parser applies.
static CMND_TAGS: &[(&str, CmndTag)] = &[
    ("NOPASSWD:", CmndTag::NoPasswd(true)),
    ("PASSWD:", CmndTag::NoPasswd(false)),
    ("NOEXEC:", CmndTag::NoExec(true)),
    ("EXEC:", CmndTag::NoExec(false)),
    ("SETENV:", CmndTag::SetEnv(true)),
    ("NOSETENV:", CmndTag::SetEnv(false)),
];

/// The effect of a command-list tag.
#[derive(Debug, Clone, Copy)]
enum CmndTag {
    NoPasswd(bool),
    NoExec(bool),
    SetEnv(bool),
}

/// Parse a comma-separated command list, handling tags like NOPASSWD:, NOEXEC:, etc.
///
/// Rejects a tag with no command after it, a tag-shaped token that is not a
/// tag, and a command list that is empty. All three used to be accepted and
/// then quietly amount to nothing: an entry with no commands grants nothing,
/// which is the safe direction but is never what the line's author meant, and
/// `visudo -c` said the file was fine.
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
        let had_tag_prefix = CMND_TAGS.iter().any(|(tag, _)| part.starts_with(tag));

        // Process tags (NOPASSWD:, PASSWD:, NOEXEC:, EXEC:, SETENV:, NOSETENV:).
        while let Some((tag, effect)) = CMND_TAGS.iter().find(|(tag, _)| part.starts_with(tag)) {
            match *effect {
                CmndTag::NoPasswd(v) => nopasswd = v,
                CmndTag::NoExec(v) => noexec = v,
                CmndTag::SetEnv(v) => setenv = v,
            }
            part = part.get(tag.len()..).unwrap_or("").trim();
        }

        if part.is_empty() {
            if had_tag_prefix {
                // `NOPASSWD:` with nothing after it. The tag applies to a
                // command; with no command it applies to nothing, and the next
                // entry in the list inherits it by accident.
                return Err(SudoError::ParseError(
                    "tag with no command after it in command list".to_string(),
                ));
            }
            continue;
        }

        // A token shaped like a tag but not in the table is a misspelling —
        // `NOPASSWORD:` for `NOPASSWD:`, most likely. Accepted, it becomes a
        // *command named* `NOPASSWORD:`, so the entry grants a program that
        // does not exist and silently still asks for a password.
        if let Some(word) = part.split_whitespace().next()
            && word.ends_with(':')
            && word
                .chars()
                .all(|c| c.is_ascii_uppercase() || c == '_' || c == ':')
        {
            return Err(SudoError::ParseError(format!(
                "unknown tag in command list: {word}"
            )));
        }

        // Split command from optional arguments. `split_once` hands back both
        // halves already past the space, so there is no `space + 1` whose
        // correctness rests on the separator being one byte wide.
        let (cmd, args) = part
            .split_once(' ')
            .map_or((part, ""), |(cmd, args)| (cmd.trim(), args.trim()));

        commands.push(CmndSpec {
            nopasswd,
            noexec,
            setenv,
            command: cmd.to_string(),
            args: args.to_string(),
        });
    }

    if commands.is_empty() {
        return Err(SudoError::ParseError(
            "privilege specification with no commands".to_string(),
        ));
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
fn validate_sudoers_line(line: &str, line_num: usize, strict: bool, errors: &mut Vec<SyntaxError>) {
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

    // Validate Defaults by *running the parser*, rather than by a separate
    // check that has to agree with it. This branch used to do neither: it
    // confirmed there was something after `Defaults` and returned, so every
    // malformed directive reached `visudo -c` and was reported as fine.
    if let Some(rest) = strip_defaults_keyword(line) {
        let mut dummy = SudoersConfig::new();
        if let Err(e) = parse_defaults(rest, &mut dummy) {
            errors.push(SyntaxError {
                line_num,
                message: e.to_string(),
                is_warning: false,
            });
            return;
        }
        // Names the parser could not check. Warnings, not errors: an unlisted
        // name may be a setting real sudo has and `KNOWN_DEFAULTS` does not.
        // `visudo` must not be able to refuse a correct file over a gap in our
        // own table — an administrator cannot fix that.
        for setting in dummy.defaults.iter().flat_map(|d| d.settings.iter()) {
            if default_shape(&setting.name).is_none() {
                errors.push(SyntaxError {
                    line_num,
                    message: format!("unknown Defaults setting '{}'", setting.name),
                    is_warning: true,
                });
            } else if strict && !HONOURED_DEFAULTS.contains(&setting.name.as_str()) {
                // Reported only under `-s`, and only for names we do recognise,
                // so it is a statement about this implementation rather than
                // about the file. Saying nothing would leave the administrator
                // believing a setting took effect that never runs.
                errors.push(SyntaxError {
                    line_num,
                    message: format!(
                        "Defaults setting '{}' is recognised but not yet honoured by this sudo",
                        setting.name
                    ),
                    is_warning: true,
                });
            }
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
    if split_at_eq_outside_parens(line).is_none() {
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
        if !user_matches(
            &priv_spec.users,
            username,
            user_groups,
            &config.user_aliases,
        ) {
            continue;
        }
        if !host_matches(&priv_spec.hosts, hostname, &config.host_aliases) {
            continue;
        }
        if !runas_matches(
            &priv_spec.runas,
            target_user,
            target_group,
            &config.runas_aliases,
        ) {
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
            && user_groups.iter().any(|g| g == group)
        {
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
                    && user_groups.iter().any(|g| g == group)
                {
                    return true;
                }
            }
        }
        // Negation.
        if let Some(negated) = spec.strip_prefix('!')
            && negated == username
        {
            return false;
        }
    }
    false
}

/// Check if a hostname matches a host specification list.
fn host_matches(specs: &[String], hostname: &str, aliases: &HashMap<String, Vec<String>>) -> bool {
    for spec in specs {
        if spec == "ALL" {
            return true;
        }
        if spec == hostname {
            return true;
        }
        if let Some(members) = aliases.get(spec.as_str())
            && members.iter().any(|m| m == hostname || m == "ALL")
        {
            return true;
        }
        if let Some(negated) = spec.strip_prefix('!')
            && negated == hostname
        {
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
            let (cmd, args) = member
                .split_once(' ')
                .map_or((member.as_str(), ""), |(cmd, args)| (cmd, args.trim()));
            if command_path_matches(cmd, actual_cmd) && (args.is_empty() || args == "*") {
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
    // `strip_suffix` rather than `ends_with` followed by a length subtraction:
    // it removes the character it names, so the two cannot disagree about how
    // much to trim.
    if let Some(dir) = spec.strip_suffix('*')
        && dir.ends_with('/')
    {
        return actual.starts_with(dir);
    }
    // Basename match: if spec has no path separator, match basename of actual.
    if !spec.contains('/')
        && let Some(base) = actual.rsplit('/').next()
    {
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
        if !user_matches(
            &priv_spec.users,
            username,
            user_groups,
            &config.user_aliases,
        ) {
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
                && let Ok(ts) = ts_str.trim().parse::<u64>()
            {
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
    fs::write(&path, format!("{now}\n"))
        .map_err(|e| SudoError::TimestampError(format!("cannot write timestamp: {e}")))?;
    Ok(())
}

/// Invalidate (expire) the timestamp for a user.
fn invalidate_timestamp(username: &str) -> Result<(), SudoError> {
    let path = timestamp_path(username);
    if path.exists() {
        // Write epoch 0 to invalidate without removing.
        fs::write(&path, "0\n")
            .map_err(|e| SudoError::TimestampError(format!("cannot invalidate timestamp: {e}")))?;
    }
    Ok(())
}

/// Remove the timestamp file entirely.
fn remove_timestamp(username: &str) -> Result<(), SudoError> {
    let path = timestamp_path(username);
    if path.exists() {
        fs::remove_file(&path)
            .map_err(|e| SudoError::TimestampError(format!("cannot remove timestamp: {e}")))?;
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
    let env_reset = config.is_default_set("env_reset") || config.get_default("env_reset").is_none();
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
                if check_list.iter().any(|k| k == &key) && (val.contains('/') || val.contains('%'))
                {
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
        set_or_replace(
            &mut env,
            "PATH",
            "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin",
        );
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
        set_or_replace(&mut env, "SUDO_GID", &format!("{}", current_gid()));
        set_or_replace(&mut env, "SUDO_UID", &format!("{}", current_uid()));
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
    // Simple epoch-based formatting (Slate OS will have its own time formatting).
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

    format!("{year:04}-{month:02}-{day:02} {hours:02}:{minutes:02}:{seconds:02}")
}

/// Convert days since epoch to (year, month, day).
fn days_to_date(mut days: u64) -> (u64, u64, u64) {
    let mut year = 1970u64;

    loop {
        let days_in_year = if is_leap_year(year) { 366 } else { 365 };
        // `checked_sub` is the loop's exit test and its subtraction at once.
        // The old form compared and then subtracted -- two statements of one
        // fact, which is the shape that lets a guard drift from the operation
        // it guards. Same below for months.
        let Some(rest) = days.checked_sub(days_in_year) else {
            break;
        };
        days = rest;
        year = year.saturating_add(1);
    }

    let leap = is_leap_year(year);
    let month_days: [u64; 12] = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];

    let mut month = 1u64;
    for &md in &month_days {
        let Some(rest) = days.checked_sub(md) else {
            break;
        };
        days = rest;
        month = month.saturating_add(1);
    }

    // Days are counted from zero within the month; calendars start at one.
    (year, month, days.saturating_add(1))
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
fn replay_session(io_dir: &str, session_id: &str, speed_factor: f64) -> Result<(), SudoError> {
    let session_dir = Path::new(io_dir).join(session_id);
    if !session_dir.is_dir() {
        return Err(SudoError::IoError(format!(
            "session directory not found: {}",
            session_dir.display()
        )));
    }

    // Read timing file.
    let timing_path = session_dir.join("timing");
    let timing_content = fs::read_to_string(&timing_path)
        .map_err(|e| SudoError::IoError(format!("cannot read timing file: {e}")))?;

    // Read stdout data.
    let stdout_path = session_dir.join("stdout");
    let stdout_data = fs::read(&stdout_path)
        .map_err(|e| SudoError::IoError(format!("cannot read stdout file: {e}")))?;

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
        // A slice pattern rather than a length test followed by three indexes:
        // the guard and the accesses were two statements of one fact, and only
        // the pattern keeps them from disagreeing.
        let [stream_text, delay_text, nbytes_text, ..] = parts.as_slice() else {
            continue;
        };

        let stream_type: u32 = match stream_text.parse() {
            Ok(t) => t,
            Err(_) => continue,
        };
        let delay_secs: f64 = match delay_text.parse() {
            Ok(d) => d,
            Err(_) => continue,
        };
        let nbytes: usize = match nbytes_text.parse() {
            Ok(n) => n,
            Err(_) => continue,
        };

        // Apply speed factor to delay.
        let adjusted_delay = delay_secs / speed_factor;
        if adjusted_delay > 0.001 {
            // Sleep for the adjusted delay.
            // On Slate OS, this would use the real sleep syscall.
            // For now, spin-wait approximation.
            let target =
                current_epoch_nanos().saturating_add((adjusted_delay * 1_000_000_000.0) as u64);
            while current_epoch_nanos() < target {
                std::hint::spin_loop();
            }
        }

        // Only replay stdout (type 1).
        if stream_type == 1 {
            let end = offset.saturating_add(nbytes).min(stdout_data.len());
            // `get` rather than a slice plus a separate `offset <` test: the
            // range is clamped above, and asking for it returns None instead of
            // panicking if a timing file ever describes bytes past the log.
            if let Some(chunk) = stdout_data.get(offset..end) {
                // Errors ignored: replay is best-effort output to a terminal
                // that may have gone away, and there is nothing to recover.
                let _ = out.write_all(chunk);
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

/// Authenticate `username` with `password`. Returns `Ok(())` only on a
/// verified match.
///
/// This used to ignore `password` entirely and return `Ok(())` for any
/// username that appeared anywhere in `/etc/users.yaml` — and, if the file did
/// not exist, for *every* username, on the reasoning that a machine with no
/// database is in single-user development mode. Both halves were a complete
/// authentication bypass in the program whose only job is to grant root, so
/// both are gone: the password is checked against the stored `crypt(3)` entry,
/// and a database that cannot be read refuses everyone.
fn authenticate(username: &str, password: &str) -> Result<(), SudoError> {
    let db = userdb::UserDb::load(userdb::DEFAULT_PATH)
        .map_err(|e| SudoError::AuthError(format!("cannot read {}: {e}", userdb::DEFAULT_PATH)))?;
    let mut auth = authlib::Authenticator::new();
    authenticate_against(&mut auth, &db, username, password)
}

/// The decision `authenticate` makes, separated from reading the file so that
/// it can be tested. The bypass this replaced survived 191 passing tests
/// precisely because the decision was welded to a path only root can write.
///
/// # The shared failed-attempt tally
///
/// Failures count against the same system-wide tally that `login`, `su`,
/// `doas`, `sshd` and the graphical greeter use, and a delay earned at any of
/// them is honoured here (`design-decisions.md` §354). Before this, guessing
/// at the prompt that grants root was the cheapest guessing on the system:
/// unlimited, untimed and unrecorded.
///
/// **Counted:** a wrong password, and a username that is not in the database.
/// The second because a tally that only ever grows for real accounts is a list
/// of which accounts are real.
///
/// **Not counted:** a locked account, an account with no password, and an
/// entry in a format nothing can recompute. No password opens any of the
/// three, so there is no guess to charge — and charging one would let anyone
/// delay that account's owner at *every* prompt on the system, for free, by
/// typing nonsense at a door that was never going to open.
fn authenticate_against(
    auth: &mut authlib::Authenticator,
    db: &userdb::UserDb,
    username: &str,
    password: &str,
) -> Result<(), SudoError> {
    // Asked before the database is consulted, so the refusal cannot be used to
    // tell "you are being slowed down" from "no such user". Asking does not
    // itself count: if it did, an attacker could hold a real user out for ever
    // with refusals that each pushed the expiry further away.
    //
    // `retry_after_secs` is discarded rather than reported. A countdown tells
    // whoever is guessing that their guesses are landing on an account that
    // exists — the graphical greeter can afford to show one because it draws
    // the user list anyway, and this prompt cannot.
    if auth.rate_limited(username).is_some() {
        return Err(SudoError::AuthError(
            authlib::Outcome::RateLimited {
                retry_after_secs: 0,
            }
            .user_message()
            .to_string(),
        ));
    }

    let Some(record) = db.find(username) else {
        auth.note_failure(username);
        return Err(SudoError::AuthError(format!(
            "user {username} not found in user database"
        )));
    };

    match record.check_password(password) {
        userdb::Auth::Accepted => {
            // The run of failures is over, here and at every other prompt.
            auth.reset(username);
            Ok(())
        }
        userdb::Auth::Locked => Err(SudoError::AuthError(format!(
            "account {username} is locked"
        ))),
        userdb::Auth::NoPassword => Err(SudoError::AuthError(format!(
            "account {username} has no password set"
        ))),
        // Named separately from a wrong password so that an administrator is
        // sent to `useradm` rather than made to hunt a password the user has
        // not in fact forgotten.
        userdb::Auth::Unusable => Err(SudoError::AuthError(format!(
            "account {username} has a password stored in a format this system \
             can no longer verify; run `useradm passwd {username}` as root"
        ))),
        userdb::Auth::Rejected => {
            auth.note_failure(username);
            Err(SudoError::AuthError(
                "incorrect password attempt".to_string(),
            ))
        }
    }
}

// ============================================================================
// Platform helpers (Slate OS stubs)
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

/// The real id of the calling process, read from `/proc/self/status`.
///
/// `field` is `Uid:` or `Gid:`; the first number on that line is the real id.
/// Falls back to the matching environment variable and then to 1000. The
/// fallback is deliberately *not* zero: an unknown caller must be treated as
/// unprivileged, because this value decides whether a password is demanded.
fn current_id_from_proc(field: &str, env_var: &str) -> u32 {
    if let Ok(content) = fs::read_to_string("/proc/self/status") {
        for line in content.lines() {
            if let Some(rest) = line.strip_prefix(field)
                && let Some(first) = rest.split_whitespace().next()
                && let Ok(id) = first.parse::<u32>()
            {
                return id;
            }
        }
    }
    std::env::var(env_var)
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1000)
}

/// Get the current uid.
fn current_uid() -> u32 {
    current_id_from_proc("Uid:", "UID")
}

/// Get the current gid.
fn current_gid() -> u32 {
    current_id_from_proc("Gid:", "GID")
}

/// Get the current tty name.
fn current_tty() -> String {
    std::env::var("TTY").unwrap_or_else(|_| "unknown".to_string())
}

/// The groups `username` belongs to, including the per-user group.
///
/// The previous version compared each line against `name: <user>`, a key no
/// writer of this file has ever emitted — it is `username:` — so the match
/// never fired and every user was reported as belonging to their own group and
/// nothing else. Since group membership is what the sudoers file authorises
/// on, that silently denied every rule written against a group.
fn get_user_groups(username: &str) -> Vec<String> {
    let mut groups = vec![username.to_string()];

    if let Ok(db) = userdb::UserDb::load(userdb::DEFAULT_PATH)
        && let Some(record) = db.find(username)
    {
        for g in record.groups() {
            if !g.is_empty() && !groups.contains(&g) {
                groups.push(g);
            }
        }
        // The administrator flag and the `wheel`/`admin` groups are two
        // spellings of one fact, and a record can carry either. Reconciling
        // them here rather than at each sudoers rule is what stops an account
        // being an administrator to the settings app and not to sudo.
        if record.is_admin() && !groups.iter().any(|g| g == "wheel") {
            groups.push("wheel".to_string());
        }
    }

    // Root is in wheel whether or not the database says so; the alternative is
    // that a damaged database locks everyone out of administering the machine.
    if username == "root" && !groups.iter().any(|g| g == "wheel") {
        groups.push("wheel".to_string());
    }

    groups
}

/// The target user's home directory and login shell.
///
/// Root's values are not special-cased away from the database any more: an
/// administrator who set root's shell had it ignored.
fn get_user_info(username: &str) -> (String, String) {
    let default_home = if username == "root" {
        "/root".to_string()
    } else {
        format!("/home/{username}")
    };
    let default_shell = "/bin/sh".to_string();

    let Ok(db) = userdb::UserDb::load(userdb::DEFAULT_PATH) else {
        return (default_home, default_shell);
    };
    let Some(record) = db.find(username) else {
        return (default_home, default_shell);
    };

    (
        record
            .home()
            .filter(|h| !h.is_empty())
            .unwrap_or(default_home),
        record.shell().unwrap_or(default_shell),
    )
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
            && elapsed.as_secs() < 300
        {
            return Err(SudoError::LockError(format!(
                "{} is locked by another process",
                path.display()
            )));
        }
        // Stale lock — remove it.
    }

    // Create the lock file with our PID.
    fs::write(&lock_path, format!("{}\n", std::process::id()))
        .map_err(|e| SudoError::LockError(format!("cannot create lock file: {e}")))?;

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

/// What a short option does when it is recognised.
///
/// A table rather than two `match` arms, because the previous code spelled the
/// same fourteen flags out twice — once for the bundled form (`-inE`) and once
/// for the standalone form (`-i -n -E`) — and the two lists had to agree by
/// hand. Two lists that must agree is the shape of the `is_admin`/`admin` bug
/// this crate was already fixed for once; here it decides which user you
/// become, so the lists are now one list.
enum ShortOption {
    /// A boolean flag: `-E`.
    Flag(fn(&mut SudoOpts)),
    /// Takes a value, either glued on (`-uroot`) or as the next argv element
    /// (`-u root`). Both forms reach the same setter through this arm.
    Value(fn(&mut SudoOpts, String)),
}

/// Map one short-option character to what it sets.
fn short_option(flag: char) -> Option<ShortOption> {
    Some(match flag {
        'u' => ShortOption::Value(|o, v| o.target_user = v),
        'g' => ShortOption::Value(|o, v| o.target_group = v),
        'p' => ShortOption::Value(|o, v| o.prompt = v),
        'i' => ShortOption::Flag(|o| o.login_shell = true),
        's' => ShortOption::Flag(|o| o.shell = true),
        'l' => ShortOption::Flag(|o| o.list = true),
        'v' => ShortOption::Flag(|o| o.validate = true),
        'k' => ShortOption::Flag(|o| o.invalidate = true),
        'K' => ShortOption::Flag(|o| o.remove_timestamp = true),
        'n' => ShortOption::Flag(|o| o.non_interactive = true),
        'b' => ShortOption::Flag(|o| o.background = true),
        'e' => ShortOption::Flag(|o| o.edit_mode = true),
        'E' => ShortOption::Flag(|o| o.preserve_env = true),
        _ => return None,
    })
}

/// Parse sudo command-line arguments.
///
/// Driven by a slice cursor rather than an index. That matters more here than
/// it looks: the old loop advanced `i` from inside the flag-bundle loop to
/// consume an option's value, so two counters shared responsibility for one
/// position and every `args[i]` after that point rested on both being right.
fn parse_sudo_args(args: &[String]) -> Result<SudoOpts, SudoError> {
    let mut opts = SudoOpts::default();
    let mut rest = args;

    while let Some((arg, tail)) = rest.split_first() {
        rest = tail;

        if arg == "--" {
            // Everything after `--` is the command, options included.
            opts.command.extend(rest.iter().cloned());
            break;
        }

        if !arg.starts_with('-') {
            // The first non-option argument starts the command, and takes the
            // remainder with it.
            opts.command.push(arg.clone());
            opts.command.extend(rest.iter().cloned());
            break;
        }

        if arg.starts_with("--") {
            return Err(SudoError::UsageError(format!("unknown option: {arg}")));
        }

        let flags = arg.strip_prefix('-').unwrap_or(arg);
        if flags.is_empty() {
            // A bare `-`, which names no option at all.
            return Err(SudoError::UsageError(format!("unknown option: {arg}")));
        }

        let mut chars = flags.chars();
        while let Some(flag) = chars.next() {
            // `chars.as_str()` is the untouched remainder of the bundle after
            // this character - exactly what `-uroot` needs, and correct for a
            // multi-byte character, which an index into `as_bytes()` was not.
            let glued = chars.as_str();
            match short_option(flag) {
                None => {
                    return Err(SudoError::UsageError(format!("unknown option: -{flag}")));
                }
                Some(ShortOption::Flag(set)) => set(&mut opts),
                Some(ShortOption::Value(set)) => {
                    let value = if glued.is_empty() {
                        let Some((next, after_next)) = rest.split_first() else {
                            return Err(SudoError::UsageError(format!(
                                "-{flag} requires an argument"
                            )));
                        };
                        rest = after_next;
                        next.clone()
                    } else {
                        glued.to_string()
                    };
                    set(&mut opts, value);
                    // The remainder of the bundle was the value.
                    break;
                }
            }
        }
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
    // A slice cursor, as in `parse_sudo_args`: `-f` takes its value from a tail
    // already proved non-empty, so there is no `i + 1` to bounds-check
    // separately from the `args[i + 1]` that follows it.
    let mut rest = args;

    while let Some((arg, tail)) = rest.split_first() {
        rest = tail;
        match arg.as_str() {
            "-c" => opts.check_only = true,
            "-s" => opts.strict = true,
            "-f" => {
                let Some((value, after_value)) = rest.split_first() else {
                    return Err(SudoError::UsageError("-f requires an argument".to_string()));
                };
                opts.file = value.clone();
                rest = after_value;
            }
            other if other.starts_with('-') => {
                return Err(SudoError::UsageError(format!("unknown option: {other}")));
            }
            other => {
                return Err(SudoError::UsageError(format!(
                    "unexpected argument: {other}"
                )));
            }
        }
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
    // A slice cursor, as in `parse_sudo_args` and `parse_visudo_args`.
    let mut rest = args;

    while let Some((arg, tail)) = rest.split_first() {
        rest = tail;
        match arg.as_str() {
            "-l" => opts.list = true,
            "-d" => {
                let Some((value, after_value)) = rest.split_first() else {
                    return Err(SudoError::UsageError("-d requires an argument".to_string()));
                };
                opts.directory = value.clone();
                rest = after_value;
            }
            "-s" => {
                let Some((value, after_value)) = rest.split_first() else {
                    return Err(SudoError::UsageError("-s requires an argument".to_string()));
                };
                rest = after_value;
                opts.speed_factor = value
                    .parse::<f64>()
                    .map_err(|_| SudoError::UsageError("invalid speed factor".to_string()))?;
                if opts.speed_factor <= 0.0 {
                    return Err(SudoError::UsageError(
                        "speed factor must be positive".to_string(),
                    ));
                }
            }
            other if other.starts_with('-') => {
                return Err(SudoError::UsageError(format!("unknown option: {other}")));
            }
            other => {
                opts.session_id = Some(other.to_string());
            }
        }
    }

    Ok(opts)
}

// ============================================================================
// Usage messages
// ============================================================================

fn print_sudo_usage() {
    eprintln!(
        "usage: sudo [-u user] [-g group] [-i] [-s] [-b] [-n] [-E] [-p prompt] [--] command [args...]"
    );
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

    // Bind the program and its arguments in the same step that proves there is
    // a program, rather than indexing `[0]` at three later points that each
    // rest on the emptiness argument above still holding. Both branches of the
    // `if` produce a non-empty vector, so this cannot fire -- but this is the
    // crate that decides which user runs what, and "cannot fire" is exactly the
    // reasoning that stops being true when the branches above are edited.
    let Some((program, program_args)) = effective_command.split_first() else {
        eprintln!("sudo: no command to execute");
        print_sudo_usage();
        return 1;
    };

    // Check authorization.
    let auth_result = check_authorization(
        &config,
        &username,
        &hostname,
        &opts.target_user,
        &opts.target_group,
        program,
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
    // On Slate OS, this would use exec() syscall to replace the process.
    // For now, we simulate with std::process::Command.
    let mut cmd = process::Command::new(program);
    cmd.args(program_args);

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
            eprintln!("sudo: unable to execute {program}: {e}");
            1
        }
    }
}

/// Load and parse the sudoers file.
fn load_sudoers() -> Result<SudoersConfig, SudoError> {
    let content = fs::read_to_string(SUDOERS_PATH)
        .map_err(|e| SudoError::InvalidConfig(format!("cannot read {SUDOERS_PATH}: {e}")))?;
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
                eprintln!("sudoedit: {username} is not allowed to edit {file} on {hostname}");
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
            // On Slate OS, this would use chmod syscall.

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
            // "e"/"E" -- and anything unrecognised, which sudo also treats as
            // edit-again rather than as a reason to discard the file -- fall
            // through to the loop's next iteration. These were two arms both
            // saying `continue`, which is one behaviour written twice.
            _ => {}
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

    // `detect_personality` takes the basename and strips `.exe` itself, so the
    // hand-rolled copy of that logic which used to stand here was a second
    // implementation of one rule -- the rule that decides whether this process
    // behaves as `sudo` or as `visudo`. Pass argv[0] through whole, and take
    // the remainder from the same split, so the two cannot disagree about
    // where the arguments begin.
    let (argv0, rest) = args
        .split_first()
        .map_or(("sudo", [].as_slice()), |(argv0, rest)| {
            (argv0.as_str(), rest)
        });
    let personality = detect_personality(argv0);

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

    // -- Authentication --
    //
    // The bug these cover was a total bypass: `authenticate` discarded the
    // password it was handed, admitted anyone whose name appeared anywhere in
    // `/etc/users.yaml`, and admitted *everyone* when that file was absent. It
    // survived every one of the tests below it because the decision was welded
    // to a filesystem path, which no test could supply.

    /// A database with one account whose password is `hunter2`.
    fn auth_fixture() -> userdb::UserDb {
        let mut db = userdb::UserDb::parse(
            "users:\n  - uid: 1000\n    username: \"alice\"\n    groups: [users, wheel]\n",
        );
        db.find_mut("alice")
            .expect("alice was just parsed")
            .set_password_with_salt("hunter2", "0123456789abcdef")
            .expect("a 16-character salt is storable");
        db
    }

    /// An `Authenticator` with no store behind it: the two paths do not exist,
    /// so no test can read the real `/etc/users.yaml` or write the real
    /// faillock file. The tally lives in memory for the life of the value,
    /// which is exactly the scope a test wants.
    fn scratch_authenticator() -> authlib::Authenticator {
        let missing = std::path::Path::new("/nonexistent/sudo-tests");
        authlib::Authenticator::with_stores(missing, missing)
    }

    /// Enough failures to be past the free allowance and unambiguously into
    /// the delayed region — used by the tests that assert a count *stays* at
    /// zero, where stopping at the allowance would prove nothing.
    const FREE_ATTEMPTS_HEADROOM: u32 = authlib::FREE_ATTEMPTS + 3;

    #[test]
    fn auth_accepts_the_right_password() {
        let db = auth_fixture();
        let mut auth = scratch_authenticator();
        assert!(authenticate_against(&mut auth, &db, "alice", "hunter2").is_ok());
    }

    #[test]
    fn auth_refuses_the_wrong_password() {
        let db = auth_fixture();
        let mut auth = scratch_authenticator();
        assert!(authenticate_against(&mut auth, &db, "alice", "Hunter2").is_err());
        assert!(authenticate_against(&mut auth, &db, "alice", "").is_err());
    }

    /// The heart of the bypass: existing in the file was treated as proof of
    /// identity.
    #[test]
    fn auth_refuses_a_known_user_with_no_password_offered() {
        let db = auth_fixture();
        let mut auth = scratch_authenticator();
        assert!(authenticate_against(&mut auth, &db, "alice", "anything at all").is_err());
    }

    #[test]
    fn auth_refuses_an_unknown_user() {
        let db = auth_fixture();
        let mut auth = scratch_authenticator();
        assert!(authenticate_against(&mut auth, &db, "mallory", "hunter2").is_err());
    }

    /// The other half of the bypass: an empty database admitted everyone.
    #[test]
    fn auth_refuses_everyone_when_the_database_is_empty() {
        let db = userdb::UserDb::new();
        let mut auth = scratch_authenticator();
        assert!(authenticate_against(&mut auth, &db, "root", "").is_err());
        assert!(authenticate_against(&mut auth, &db, "alice", "hunter2").is_err());
    }

    #[test]
    fn auth_refuses_a_locked_account_that_knows_its_password() {
        let mut db = auth_fixture();
        db.find_mut("alice").expect("alice exists").set_locked(true);
        let mut auth = scratch_authenticator();
        assert!(authenticate_against(&mut auth, &db, "alice", "hunter2").is_err());
    }

    /// A username that is a substring of another must not authenticate as it.
    /// The replaced code searched the file text for `name: <user>`, which
    /// matched the tail of `username: <user>` — and would equally have matched
    /// a display name, a home directory or a comment.
    #[test]
    fn auth_does_not_match_a_username_by_substring() {
        let mut db = userdb::UserDb::parse(
            "users:\n  - uid: 1000\n    username: \"alice\"\n    \
             display_name: \"al\"\n    home_dir: \"/home/al\"\n",
        );
        db.find_mut("alice")
            .expect("alice was just parsed")
            .set_password_with_salt("hunter2", "0123456789abcdef")
            .expect("a 16-character salt is storable");
        let mut auth = scratch_authenticator();
        assert!(authenticate_against(&mut auth, &db, "al", "hunter2").is_err());
    }

    // -- The shared failed-attempt tally (`design-decisions.md` §354) --
    //
    // Before these, this prompt — the one that hands out root — was the one
    // place on the system where guessing was free: unlimited, untimed, and
    // invisible to every other prompt's limit.

    /// A wrong password here is a guess, and is charged like a guess anywhere
    /// else. The delay is not local to `sudo`: it is the same tally `login`,
    /// `su` and the greeter read, so an attacker cannot walk from one prompt to
    /// the next to keep guessing at full speed.
    #[test]
    fn a_wrong_password_is_charged_to_the_shared_tally() {
        let db = auth_fixture();
        let mut auth = scratch_authenticator();

        for expected in 1..=authlib::FREE_ATTEMPTS {
            assert!(authenticate_against(&mut auth, &db, "alice", "wrong").is_err());
            assert_eq!(auth.failures("alice"), expected);
        }

        // The allowance is spent; the next refusal comes with a wait.
        assert!(authenticate_against(&mut auth, &db, "alice", "wrong").is_err());
        assert!(auth.rate_limited("alice").is_some());
    }

    /// A name nobody holds is counted too. If it were not, the only accounts
    /// ever slowed down would be the real ones — and the delay would then be a
    /// working answer to "does this user exist?", asked one name at a time.
    #[test]
    fn an_unknown_username_is_counted_the_same_as_a_wrong_password() {
        let db = auth_fixture();
        let mut auth = scratch_authenticator();

        assert!(authenticate_against(&mut auth, &db, "mallory", "hunter2").is_err());
        assert_eq!(auth.failures("mallory"), 1);
    }

    /// Three doors that no password opens: locked, no password stored, and a
    /// hash in a format nothing can recompute. Guessing at any of them cannot
    /// succeed, so there is no guess to charge — and charging one would hand
    /// anybody a free way to keep the account's real owner waiting at *every*
    /// prompt on the system, by typing nonsense at a door already shut.
    #[test]
    fn a_refusal_that_no_password_could_have_passed_is_not_an_attempt() {
        let mut auth = scratch_authenticator();

        let mut locked = auth_fixture();
        locked.find_mut("alice").expect("alice exists").set_locked(true);
        for _ in 0..FREE_ATTEMPTS_HEADROOM {
            assert!(authenticate_against(&mut auth, &locked, "alice", "hunter2").is_err());
        }
        assert_eq!(auth.failures("alice"), 0);
        assert!(auth.rate_limited("alice").is_none());

        // An account with nothing stored in the password field at all.
        let bare = userdb::UserDb::parse(
            "users:\n  - uid: 1001\n    username: \"bob\"\n    groups: [users]\n",
        );
        for _ in 0..FREE_ATTEMPTS_HEADROOM {
            assert!(authenticate_against(&mut auth, &bare, "bob", "anything").is_err());
        }
        assert_eq!(auth.failures("bob"), 0);
    }

    /// The right password ends the run — here and everywhere else, because the
    /// tally is one tally. A user who mistypes twice and then succeeds is not
    /// left carrying those two failures into the next prompt they meet.
    #[test]
    fn success_clears_the_count() {
        let db = auth_fixture();
        let mut auth = scratch_authenticator();

        assert!(authenticate_against(&mut auth, &db, "alice", "wrong").is_err());
        assert!(authenticate_against(&mut auth, &db, "alice", "wrong").is_err());
        assert_eq!(auth.failures("alice"), 2);

        assert!(authenticate_against(&mut auth, &db, "alice", "hunter2").is_ok());
        assert_eq!(auth.failures("alice"), 0);
    }

    /// Once the wait is running, sudo refuses without looking at the database —
    /// and the refusal itself is not counted. Counting it would let an attacker
    /// hold a real user out indefinitely by hammering a prompt they already
    /// know is closed, each refusal pushing the expiry further away.
    #[test]
    fn a_delayed_user_is_refused_and_the_refusal_is_not_counted() {
        let db = auth_fixture();
        let mut auth = scratch_authenticator();

        while auth.rate_limited("alice").is_none() {
            assert!(authenticate_against(&mut auth, &db, "alice", "wrong").is_err());
        }
        let counted = auth.failures("alice");

        // Even the *correct* password does not skip the wait.
        for _ in 0..FREE_ATTEMPTS_HEADROOM {
            assert!(authenticate_against(&mut auth, &db, "alice", "hunter2").is_err());
        }
        assert_eq!(auth.failures("alice"), counted);
    }

    /// The refusal a delayed caller gets says only that they are being slowed
    /// down — never how long is left, and never whether the name exists. A
    /// countdown at a text prompt is a working oracle: it tells whoever is
    /// guessing that their guesses are landing on a real account.
    #[test]
    fn the_delayed_refusal_discloses_neither_the_account_nor_the_countdown() {
        let db = auth_fixture();
        let mut auth = scratch_authenticator();

        while auth.rate_limited("alice").is_none() {
            assert!(authenticate_against(&mut auth, &db, "alice", "wrong").is_err());
        }

        let SudoError::AuthError(message) =
            authenticate_against(&mut auth, &db, "alice", "hunter2")
                .expect_err("the wait is running")
        else {
            panic!("a rate-limited refusal is an authentication error");
        };
        assert!(!message.contains("alice"), "leaked the username: {message}");
        assert!(
            !message.chars().any(|c| c.is_ascii_digit()),
            "leaked a countdown: {message}"
        );
    }

    /// The delay is checked before the database is, so the two refusals a
    /// probing caller can tell apart — "no such user" and "wrong password" —
    /// both collapse into the same rate-limited message once the wait is
    /// running. Otherwise the limit would itself become the oracle it exists
    /// to protect.
    #[test]
    fn the_delay_is_checked_before_any_account_state_is_disclosed() {
        let db = auth_fixture();
        let mut auth = scratch_authenticator();

        while auth.rate_limited("mallory").is_none() {
            assert!(authenticate_against(&mut auth, &db, "mallory", "guess").is_err());
        }

        let SudoError::AuthError(message) =
            authenticate_against(&mut auth, &db, "mallory", "guess")
                .expect_err("the wait is running")
        else {
            panic!("a rate-limited refusal is an authentication error");
        };
        assert!(
            !message.contains("not found"),
            "the limit disclosed what it was meant to hide: {message}"
        );
    }

    /// The point of the whole exercise: a delay earned at another prompt is
    /// honoured here. `Authenticator::with_faillock` is what `login`, `su` and
    /// the greeter share on a real system; two authenticators pointed at one
    /// file stand in for two programs.
    #[test]
    fn sudo_honours_a_delay_earned_at_another_prompt() {
        let dir = std::env::temp_dir().join("sudo-faillock-share-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a scratch directory under the temp dir");
        let faillock = dir.join("faillock");
        let missing = std::path::Path::new("/nonexistent/sudo-tests");

        // `login` (or `su`, or the greeter) burns through the allowance.
        {
            let mut elsewhere =
                authlib::Authenticator::with_stores(missing, missing).with_faillock(&faillock);
            while elsewhere.rate_limited("alice").is_none() {
                elsewhere.note_failure("alice");
            }
        }

        // `sudo` starts fresh, reads the same file, and refuses.
        let db = auth_fixture();
        let mut auth =
            authlib::Authenticator::with_stores(missing, missing).with_faillock(&faillock);
        assert!(
            auth.rate_limited("alice").is_some(),
            "a delay earned at another prompt must be honoured here"
        );
        assert!(
            authenticate_against(&mut auth, &db, "alice", "hunter2").is_err(),
            "the correct password must not skip a wait earned elsewhere"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

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
        assert_eq!(detect_personality("C:\\Windows\\sudo"), Personality::Sudo);
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
        assert_eq!(detect_personality("sudoedit.exe"), Personality::Sudoedit);
    }

    #[test]
    fn personality_detect_visudo() {
        assert_eq!(detect_personality("visudo"), Personality::Visudo);
    }

    #[test]
    fn personality_detect_visudo_path() {
        assert_eq!(detect_personality("/usr/sbin/visudo"), Personality::Visudo);
    }

    #[test]
    fn personality_detect_visudo_exe() {
        assert_eq!(detect_personality("visudo.exe"), Personality::Visudo);
    }

    #[test]
    fn personality_detect_sudoreplay() {
        assert_eq!(detect_personality("sudoreplay"), Personality::Sudoreplay);
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
    fn personality_detect_handles_a_non_ascii_path() {
        // Our paths allow every byte but `/` and NUL, so a directory name can
        // hold multi-byte characters. The previous scan produced a byte index
        // and sliced the `&str` at it, which is a panic on a boundary that is
        // not a character boundary -- it survived only because it was always
        // reached via a `/`. `visudo` here is the whole point: this decides
        // whether the process edits the policy file or grants root.
        assert_eq!(
            detect_personality("/usr/sbin/\u{e9}t\u{e9}/visudo"),
            Personality::Visudo
        );
        assert_eq!(detect_personality("\u{e9}sudo"), Personality::Sudo);
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
            &vec![
                "alice".to_string(),
                "bob".to_string(),
                "charlie".to_string()
            ]
        );
    }

    #[test]
    fn parse_host_alias() {
        let config = parse_sudoers("Host_Alias SERVERS = web1, web2, db1\n").unwrap();
        assert_eq!(
            config.host_aliases.get("SERVERS").unwrap(),
            &vec!["web1".to_string(), "web2".to_string(), "db1".to_string()]
        );
    }

    #[test]
    fn parse_cmnd_alias() {
        let config =
            parse_sudoers("Cmnd_Alias NETWORKING = /sbin/ifconfig, /sbin/route, /sbin/iptables\n")
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
            parse_sudoers("User_Alias ADMINS = alice, bob : DEVS = charlie, dave\n").unwrap();
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
        let config = parse_sudoers("Defaults env_keep=\"SSH_AUTH_SOCK DISPLAY\"\n").unwrap();
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

    // -- Defaults grammar: the malformed directives that used to be accepted --
    //
    // Each case below reached `visudo -c` before this and was reported as a
    // valid file. The point of the group is not that any one of them is likely,
    // but that `visudo`'s whole job is to catch them before the file is
    // installed, and it caught none of them.

    #[test]
    fn defaults_plus_equals_keeps_the_name_clean() {
        // The defect the operator-aware parse exists for. `env_keep += "X"` was
        // split at the first `=`, so the name became `env_keep +`. Two consumers
        // matched three spellings each to work around it; `get_default` matched
        // one, so no `+=` line was ever visible to `timestamp_timeout` or
        // `env_reset`.
        let config = parse_sudoers("Defaults env_keep += \"FOO BAR\"\n").unwrap();
        let setting = &config.defaults[0].settings[0];
        assert_eq!(setting.name, "env_keep");
        assert_eq!(setting.op, DefaultOp::Add);
        assert_eq!(setting.value, "FOO BAR");
    }

    #[test]
    fn defaults_env_keep_assignment_replaces_and_add_appends() {
        // `=` and `+=` mean different things, and the difference is the whole
        // reason a file writes one rather than the other: `=` narrows the kept
        // environment to exactly what is listed. The old code could not see the
        // `+=` form at all and so treated both as "add" -- a file that meant to
        // narrow did not narrow, which is the unsafe direction.
        let replaced = parse_sudoers("Defaults env_keep = \"ONLY_THIS\"\n").unwrap();
        assert_eq!(replaced.env_keep_list(), vec!["ONLY_THIS".to_string()]);

        let appended = parse_sudoers("Defaults env_keep += \"EXTRA\"\n").unwrap();
        let keep = appended.env_keep_list();
        assert!(keep.contains(&"EXTRA".to_string()));
        assert!(keep.len() > 1, "built-ins should survive `+=`: {keep:?}");
    }

    #[test]
    fn defaults_env_keep_minus_equals_removes() {
        let config =
            parse_sudoers("Defaults env_keep = \"A B C\"\nDefaults env_keep -= \"B\"\n").unwrap();
        assert_eq!(
            config.env_keep_list(),
            vec!["A".to_string(), "C".to_string()]
        );
    }

    #[test]
    fn defaults_later_line_wins() {
        // `get_default` returned the *first* match, so a file that overrode a
        // setting further down kept the earlier value -- the opposite of what
        // reading the file top to bottom tells you.
        let config =
            parse_sudoers("Defaults timestamp_timeout=5\nDefaults timestamp_timeout=30\n").unwrap();
        assert_eq!(config.get_default("timestamp_timeout"), Some("30"));
    }

    #[test]
    fn defaults_flag_with_a_value_is_rejected() {
        assert!(parse_sudoers("Defaults requiretty=5\n").is_err());
    }

    #[test]
    fn defaults_value_setting_without_a_value_is_rejected() {
        // `Defaults timestamp_timeout` alone used to be stored as the boolean
        // `timestamp_timeout=true`, which then failed to parse as a number and
        // fell back to the built-in timeout. Silently.
        assert!(parse_sudoers("Defaults timestamp_timeout\n").is_err());
    }

    #[test]
    fn defaults_value_setting_cannot_be_negated() {
        assert!(parse_sudoers("Defaults !secure_path\n").is_err());
    }

    #[test]
    fn defaults_plus_equals_on_a_single_valued_setting_is_rejected() {
        assert!(parse_sudoers("Defaults secure_path += /usr/local/bin\n").is_err());
    }

    #[test]
    fn defaults_negated_with_a_value_is_rejected() {
        assert!(parse_sudoers("Defaults !env_reset=1\n").is_err());
    }

    #[test]
    fn defaults_empty_name_is_rejected() {
        assert!(parse_sudoers("Defaults =5\n").is_err());
        assert!(parse_sudoers("Defaults !\n").is_err());
    }

    #[test]
    fn defaults_name_with_whitespace_is_rejected() {
        // Almost always a forgotten comma between two settings.
        assert!(parse_sudoers("Defaults passwd tries=3\n").is_err());
    }

    #[test]
    fn defaults_unterminated_quote_is_rejected() {
        // `trim_matches('"')` swallowed this: the value became `A B` and the
        // file looked fine.
        assert!(parse_sudoers("Defaults env_keep = \"A B\n").is_err());
    }

    #[test]
    fn defaults_scope_with_no_settings_is_rejected() {
        // Used to return `Ok(())` and discard the line, so this looked to its
        // author like it had restricted something for alice.
        assert!(parse_sudoers("Defaults:alice\n").is_err());
    }

    #[test]
    fn a_user_whose_name_starts_with_defaults_is_not_a_directive() {
        // `strip_prefix("Defaults")` fires on this too, and the remainder
        // (`foo ALL = ALL`) then reads as a settings list with a space in the
        // name -- an error, in a file that is entirely correct. Harmless while
        // nothing was ever rejected; a validator that refuses a correct file is
        // worse than one that misses a wrong one, because the administrator
        // cannot act on it.
        let config = parse_sudoers("Defaultsfoo ALL = /bin/ls\n").unwrap();
        assert_eq!(config.privileges.len(), 1);
        assert_eq!(config.privileges[0].users, vec!["Defaultsfoo"]);
        assert!(config.defaults.is_empty());
        assert!(validate_sudoers("Defaultsfoo ALL = /bin/ls\n", true).is_empty());
    }

    #[test]
    fn defaults_unknown_name_still_parses() {
        // Deliberate: `KNOWN_DEFAULTS` is incomplete, so an unrecognised name is
        // not evidence of an error. `visudo` reports it as a warning instead --
        // see `validate_unknown_defaults_setting_is_a_warning`.
        let config = parse_sudoers("Defaults some_future_sudo_setting=1\n").unwrap();
        assert_eq!(
            config.get_default("some_future_sudo_setting"),
            Some("1"),
            "an unknown name must still round-trip"
        );
    }

    #[test]
    fn honoured_defaults_are_all_known() {
        // Two hand-maintained lists that have to agree is this tree's recurring
        // defect shape. A name in `HONOURED_DEFAULTS` that is absent from
        // `KNOWN_DEFAULTS` would be warned about as unknown by the very
        // validator that is supposed to vouch for it.
        for name in HONOURED_DEFAULTS {
            assert!(
                default_shape(name).is_some(),
                "{name} is honoured but missing from KNOWN_DEFAULTS"
            );
        }
    }

    // -- Command lists --

    #[test]
    fn cmnd_list_tag_with_no_command_is_rejected() {
        // The tag applies to a command; with none, it applies to nothing and
        // the next entry inherits it by accident.
        assert!(parse_sudoers("alice ALL = NOPASSWD:\n").is_err());
    }

    #[test]
    fn cmnd_list_unknown_tag_is_rejected() {
        // Accepted, `NOPASSWORD:` becomes a *command name*, so the entry grants
        // a program that does not exist and still asks for a password -- the
        // opposite of what its author read it as doing, and silent.
        assert!(parse_sudoers("alice ALL = NOPASSWORD: /bin/ls\n").is_err());
    }

    #[test]
    fn cmnd_list_empty_is_rejected() {
        assert!(parse_sudoers("alice ALL = \n").is_err());
    }

    #[test]
    fn cmnd_list_known_tags_still_parse() {
        let config = parse_sudoers("alice ALL = NOPASSWD: NOEXEC: /bin/ls\n").unwrap();
        let cmd = &config.privileges[0].commands[0];
        assert!(cmd.nopasswd);
        assert!(cmd.noexec);
        assert_eq!(cmd.command, "/bin/ls");
    }

    #[test]
    fn cmnd_list_uppercase_alias_member_is_not_mistaken_for_a_tag() {
        // A `Cmnd_Alias` name looks like a tag apart from the trailing colon,
        // which is why the tag check requires one.
        let config = parse_sudoers("alice ALL = NETWORKING\n").unwrap();
        assert_eq!(config.privileges[0].commands[0].command, "NETWORKING");
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
        let config = parse_sudoers("alice ALL = (root) NOPASSWD: /usr/bin/apt\n").unwrap();
        assert!(config.privileges[0].commands[0].nopasswd);
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
        let config =
            parse_sudoers("alice ALL = (root) NOPASSWD: /usr/bin/apt, PASSWD: /usr/bin/rm\n")
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
        let config = parse_sudoers("alice ALL = (bob : www-data) /usr/bin/service\n").unwrap();
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
        let config = parse_sudoers("User_Alias ADMINS = alice, \\\n    bob, charlie\n").unwrap();
        assert_eq!(config.user_aliases.get("ADMINS").unwrap().len(), 3);
    }

    #[test]
    fn parse_noexec_tag() {
        let config = parse_sudoers("alice ALL = (root) NOEXEC: /usr/bin/vi\n").unwrap();
        assert!(config.privileges[0].commands[0].noexec);
    }

    #[test]
    fn parse_setenv_tag() {
        let config = parse_sudoers("alice ALL = (root) SETENV: /usr/bin/env\n").unwrap();
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
            &config,
            "root",
            "localhost",
            "root",
            "",
            "/usr/bin/ls",
            &["root".to_string()],
        );
        assert!(result.is_some());
    }

    #[test]
    fn auth_user_not_authorized() {
        let config = parse_sudoers("root ALL = (ALL) ALL\n").unwrap();
        let result = check_authorization(
            &config,
            "alice",
            "localhost",
            "root",
            "",
            "/usr/bin/ls",
            &["alice".to_string()],
        );
        assert!(result.is_none());
    }

    #[test]
    fn auth_user_specific_command() {
        let config = parse_sudoers("alice ALL = (root) /usr/bin/apt\n").unwrap();
        let result = check_authorization(
            &config,
            "alice",
            "localhost",
            "root",
            "",
            "/usr/bin/apt",
            &["alice".to_string()],
        );
        assert!(result.is_some());
    }

    #[test]
    fn auth_user_wrong_command() {
        let config = parse_sudoers("alice ALL = (root) /usr/bin/apt\n").unwrap();
        let result = check_authorization(
            &config,
            "alice",
            "localhost",
            "root",
            "",
            "/usr/bin/rm",
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
        let config =
            parse_sudoers("User_Alias ADMINS = alice, bob\nADMINS ALL = (ALL) ALL\n").unwrap();
        let result = check_authorization(
            &config,
            "alice",
            "localhost",
            "root",
            "",
            "/usr/bin/ls",
            &["alice".to_string()],
        );
        assert!(result.is_some());
    }

    #[test]
    fn auth_host_mismatch() {
        let config = parse_sudoers("alice web1 = (root) ALL\n").unwrap();
        let result = check_authorization(
            &config,
            "alice",
            "db1",
            "root",
            "",
            "/usr/bin/ls",
            &["alice".to_string()],
        );
        assert!(result.is_none());
    }

    #[test]
    fn auth_host_match() {
        let config = parse_sudoers("alice web1 = (root) ALL\n").unwrap();
        let result = check_authorization(
            &config,
            "alice",
            "web1",
            "root",
            "",
            "/usr/bin/ls",
            &["alice".to_string()],
        );
        assert!(result.is_some());
    }

    #[test]
    fn auth_host_alias() {
        let config =
            parse_sudoers("Host_Alias SERVERS = web1, web2\nalice SERVERS = (root) ALL\n").unwrap();
        let result = check_authorization(
            &config,
            "alice",
            "web2",
            "root",
            "",
            "/usr/bin/ls",
            &["alice".to_string()],
        );
        assert!(result.is_some());
    }

    #[test]
    fn auth_runas_mismatch() {
        let config = parse_sudoers("alice ALL = (bob) ALL\n").unwrap();
        let result = check_authorization(
            &config,
            "alice",
            "localhost",
            "root",
            "",
            "/usr/bin/ls",
            &["alice".to_string()],
        );
        assert!(result.is_none());
    }

    #[test]
    fn auth_cmnd_alias() {
        let config =
            parse_sudoers("Cmnd_Alias NET = /sbin/ifconfig, /sbin/route\nalice ALL = (root) NET\n")
                .unwrap();
        let result = check_authorization(
            &config,
            "alice",
            "localhost",
            "root",
            "",
            "/sbin/ifconfig",
            &["alice".to_string()],
        );
        assert!(result.is_some());
    }

    #[test]
    fn auth_nopasswd_flag() {
        let config = parse_sudoers("alice ALL = (root) NOPASSWD: /usr/bin/apt\n").unwrap();
        let spec = check_authorization(
            &config,
            "alice",
            "localhost",
            "root",
            "",
            "/usr/bin/apt",
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
            &config,
            "alice",
            "localhost",
            "root",
            "",
            "/usr/bin/ls",
            &["alice".to_string()],
        );
        assert!(spec.is_some());
        assert!(spec.unwrap().nopasswd);
    }

    #[test]
    fn auth_wildcard_command() {
        let config = parse_sudoers("alice ALL = (root) /usr/bin/*\n").unwrap();
        let result = check_authorization(
            &config,
            "alice",
            "localhost",
            "root",
            "",
            "/usr/bin/anything",
            &["alice".to_string()],
        );
        assert!(result.is_some());
    }

    #[test]
    fn auth_wildcard_no_match_different_dir() {
        let config = parse_sudoers("alice ALL = (root) /usr/bin/*\n").unwrap();
        let result = check_authorization(
            &config,
            "alice",
            "localhost",
            "root",
            "",
            "/usr/sbin/something",
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
        assert!(!command_matches(
            "!/usr/bin/rm",
            "",
            "/usr/bin/rm",
            &aliases
        ));
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
        assert!(user_matches(&["alice".to_string()], "alice", &[], &aliases,));
    }

    #[test]
    fn user_match_all() {
        let aliases = HashMap::new();
        assert!(user_matches(&["ALL".to_string()], "anyone", &[], &aliases,));
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
        assert!(!user_matches(&["bob".to_string()], "alice", &[], &aliases,));
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
        assert!(host_matches(&["web1".to_string()], "web1", &aliases,));
    }

    #[test]
    fn host_match_all() {
        let aliases = HashMap::new();
        assert!(host_matches(&["ALL".to_string()], "anything", &aliases,));
    }

    #[test]
    fn host_no_match() {
        let aliases = HashMap::new();
        assert!(!host_matches(&["web1".to_string()], "db1", &aliases,));
    }

    #[test]
    fn host_match_via_alias() {
        let mut aliases = HashMap::new();
        aliases.insert(
            "SERVERS".to_string(),
            vec!["web1".to_string(), "web2".to_string()],
        );
        assert!(host_matches(&["SERVERS".to_string()], "web2", &aliases,));
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
        // Changed 2026-08-18. This used to assert that `env_keep="CUSTOM_VAR"`
        // kept `TERM` as well -- that is, that `=` *adds* to the built-in list.
        // It does not: sudoers documents `=` as replace and `+=` as add, and
        // the difference is the whole reason a file writes one rather than the
        // other. Asserting the old behaviour meant asserting that a file which
        // deliberately narrowed the kept environment did not narrow it, which
        // is the unsafe direction to be wrong in. The `+=` form, which is what
        // the test's name describes, is asserted below.
        let replaced = parse_sudoers("Defaults env_keep=\"CUSTOM_VAR\"\n").unwrap();
        assert_eq!(
            replaced.env_keep_list(),
            vec!["CUSTOM_VAR".to_string()],
            "`=` replaces the list"
        );

        let extended = parse_sudoers("Defaults env_keep+=\"CUSTOM_VAR\"\n").unwrap();
        let keep = extended.env_keep_list();
        assert!(keep.contains(&"CUSTOM_VAR".to_string()));
        assert!(keep.contains(&"TERM".to_string()), "`+=` keeps the built-ins");
    }

    #[test]
    fn env_keep_can_be_disabled_with_bang() {
        // `!` is sudoers' disable operator for list settings; it is the one
        // place a list may legally be written without a value.
        let config = parse_sudoers("Defaults !env_keep\n").unwrap();
        assert!(config.env_keep_list().is_empty());
    }

    #[test]
    fn defaults_command_scope_needs_no_space_before_the_sigil() {
        // The only thing separating a command-scoped default from a negated
        // global flag is whether the sigil is attached to the keyword. Reading
        // `Defaults !requiretty` as a scope -- which trimming before the sigil
        // test does -- swallows the entire space of negated global flags.
        let scoped = parse_sudoers("Defaults!/usr/bin/less !env_reset\n").unwrap();
        assert_eq!(scoped.defaults[0].scope, "!/usr/bin/less");

        let global = parse_sudoers("Defaults !env_reset\n").unwrap();
        assert!(global.defaults[0].scope.is_empty());
        assert!(!global.is_default_set("env_reset"));
    }

    #[test]
    fn env_check_empty_by_default() {
        let config = SudoersConfig::new();
        assert!(config.env_check_list().is_empty());
    }

    #[test]
    fn env_check_parsed() {
        let config = parse_sudoers("Defaults env_check=\"LD_LIBRARY_PATH\"\n").unwrap();
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
        let args = vec!["-g".to_string(), "staff".to_string(), "ls".to_string()];
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
        let args = vec!["-p".to_string(), "Enter: ".to_string(), "ls".to_string()];
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

    // The cases below are the ones the single option table exists for: each is a
    // place where the bundled form and the standalone form previously ran through
    // separate code that had to agree by hand.

    #[test]
    fn a_value_glued_to_its_flag_reaches_the_same_field_as_a_separate_one() {
        let glued = parse_sudo_args(&["-uoperator".to_string()]).unwrap();
        let separate = parse_sudo_args(&["-u".to_string(), "operator".to_string()]).unwrap();
        assert_eq!(glued.target_user, "operator");
        assert_eq!(glued.target_user, separate.target_user);
    }

    #[test]
    fn a_value_flag_ending_a_bundle_takes_the_next_argument() {
        let args = ["-nu".to_string(), "operator".to_string(), "id".to_string()];
        let opts = parse_sudo_args(&args).unwrap();
        assert!(opts.non_interactive);
        assert_eq!(opts.target_user, "operator");
        assert_eq!(opts.command, vec!["id".to_string()]);
    }

    #[test]
    fn a_value_flag_mid_bundle_swallows_the_rest_of_it() {
        // `-nuoperator` is `-n` plus `-u operator`, not `-n -u -o -p -e ...`.
        let opts = parse_sudo_args(&["-nuoperator".to_string()]).unwrap();
        assert!(opts.non_interactive);
        assert_eq!(opts.target_user, "operator");
    }

    #[test]
    fn a_bundle_missing_its_trailing_value_is_an_error() {
        assert!(parse_sudo_args(&["-nu".to_string()]).is_err());
    }

    #[test]
    fn a_bare_dash_and_a_long_option_are_both_rejected() {
        assert!(parse_sudo_args(&["-".to_string()]).is_err());
        assert!(parse_sudo_args(&["--frobnicate".to_string()]).is_err());
    }

    #[test]
    fn an_option_after_the_command_belongs_to_the_command() {
        let args = ["id".to_string(), "-u".to_string()];
        let opts = parse_sudo_args(&args).unwrap();
        assert_eq!(opts.command, vec!["id".to_string(), "-u".to_string()]);
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

    // -- visudo now actually validates Defaults --
    //
    // Every case here reported a clean file before. The `Defaults` branch
    // confirmed there was something after the keyword and returned, so the one
    // tool whose job is to catch the administrator's mistake before the policy
    // is installed caught none of them.

    #[test]
    fn validate_malformed_defaults_is_an_error() {
        for content in [
            "Defaults requiretty=5\n",
            "Defaults !secure_path\n",
            "Defaults passwd tries=3\n",
            "Defaults env_keep = \"A B\n",
            "Defaults:alice\n",
        ] {
            let errors = validate_sudoers(content, false);
            assert!(
                errors.iter().any(|e| !e.is_warning),
                "no error reported for {content:?}"
            );
        }
    }

    #[test]
    fn validate_unknown_defaults_setting_is_a_warning() {
        // A warning rather than an error on purpose: `KNOWN_DEFAULTS` is
        // knowingly incomplete, and a `visudo` that refused to save a correct
        // file over a gap in our own table would leave the administrator with
        // no way to fix it.
        let errors = validate_sudoers("Defaults some_future_sudo_setting=1\n", false);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].is_warning);
        assert!(errors[0].message.contains("some_future_sudo_setting"));
    }

    #[test]
    fn validate_recognised_but_unhonoured_setting_warns_only_under_strict() {
        // `Defaults requiretty` parses, means something in real sudo, and does
        // nothing here. Saying so is the honest report; saying it always would
        // make `visudo -c` noisy on files that are entirely correct.
        assert!(validate_sudoers("Defaults requiretty\n", false).is_empty());
        let strict = validate_sudoers("Defaults requiretty\n", true);
        assert_eq!(strict.len(), 1);
        assert!(strict[0].is_warning);
        assert!(strict[0].message.contains("not yet honoured"));
    }

    #[test]
    fn validate_honoured_defaults_are_silent_even_under_strict() {
        assert!(validate_sudoers("Defaults env_reset\n", true).is_empty());
        assert!(validate_sudoers("Defaults timestamp_timeout=15\n", true).is_empty());
        assert!(validate_sudoers("Defaults env_keep += \"DISPLAY\"\n", true).is_empty());
    }

    #[test]
    fn validate_malformed_command_list_is_an_error() {
        for content in ["alice ALL = NOPASSWD:\n", "alice ALL = NOPASSWORD: /bin/ls\n"] {
            let errors = validate_sudoers(content, false);
            assert!(
                errors.iter().any(|e| !e.is_warning),
                "no error reported for {content:?}"
            );
        }
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
        let config = parse_sudoers("alice ALL = (root) NOPASSWD: /usr/bin/apt\n").unwrap();
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

    // -- split_at_eq_outside_parens tests --
    //
    // These assert the two halves rather than the index, which is what callers
    // actually consume. The index form let a test pass while the caller's own
    // `+ 1` was the thing that decided whether the `=` ended up in the
    // right-hand side.

    #[test]
    fn find_eq_simple() {
        assert_eq!(split_at_eq_outside_parens("a = b"), Some(("a ", " b")));
    }

    #[test]
    fn find_eq_inside_parens() {
        assert_eq!(
            split_at_eq_outside_parens("a (x=y) = b"),
            Some(("a (x=y) ", " b"))
        );
    }

    #[test]
    fn find_eq_no_eq() {
        assert_eq!(split_at_eq_outside_parens("no equals here"), None);
    }

    #[test]
    fn find_eq_nested_parens() {
        assert_eq!(
            split_at_eq_outside_parens("a ((x=y)) = b"),
            Some(("a ((x=y)) ", " b"))
        );
    }

    #[test]
    fn find_eq_keeps_the_equals_out_of_both_halves() {
        // The `=` belongs to neither side. An off-by-one in the old caller
        // would have left it leading the right half, and a `Defaults` setting
        // would then have parsed `=value` as its value.
        assert_eq!(split_at_eq_outside_parens("k=v"), Some(("k", "v")));
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
