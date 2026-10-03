//! The sudoers file, as `sudo` and `visudo` both read it: its data model, its
//! parser, its syntax check, and the editor either program hands a file to.
//!
//! A library because two programs need it and they are two programs on
//! purpose. Upstream builds `visudo` apart from `sudo` from one source tree,
//! and so does this package (design-decisions 1045): `sudo` needs the right to
//! change who a command runs as, and `visudo`, which only edits a root-owned
//! file, needs none of it -- a binary that answered to both names would hold
//! the union. `sudoedit` stays a name of `sudo`, as it is upstream.

#![deny(clippy::all)]

use quoting::{os_bytes, os_from_bytes, quoteaf_os};
use std::collections::HashMap;
use std::env;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::io;

pub const SUDOERS_PATH: &str = "/etc/sudoers";
pub const DEFAULT_EDITOR: &str = "/usr/bin/vi";
pub const DEFAULT_TIMEOUT: u64 = 900; // 15 minutes in seconds
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

// ============================================================================
// Error types
// ============================================================================

/// Unified error type for sudo operations.
#[derive(Debug)]
pub enum SudoError {
    _PermissionDenied(String),
    ParseError(String),
    IoError(String),
    InvalidConfig(String),
    AuthError(String),
    UsageError(String),
    TimestampError(String),
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
pub enum DefaultShape {
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
pub enum DefaultOp {
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
pub struct DefaultSetting {
    /// The setting name, with no operator attached.
    pub name: String,
    /// How it was written.
    pub op: DefaultOp,
    /// The value; empty for `Flag` settings, whose truth is carried by `op`.
    pub value: String,
}

/// A Defaults directive from the sudoers file.
#[derive(Debug, Clone)]
pub struct DefaultsDirective {
    /// The scope (empty = global, "user:" prefix, "host:" prefix, etc.)
    pub scope: String,
    /// The settings on this line, in written order.
    pub settings: Vec<DefaultSetting>,
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
pub static KNOWN_DEFAULTS: &[(&str, DefaultShape)] = &[
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
pub static HONOURED_DEFAULTS: &[&str] = &[
    "env_check",
    "env_keep",
    "env_reset",
    "secure_path",
    "timestamp_timeout",
];

/// Look up a setting's shape, or `None` if the name is not in [`KNOWN_DEFAULTS`].
pub fn default_shape(name: &str) -> Option<DefaultShape> {
    KNOWN_DEFAULTS
        .iter()
        .find(|(known, _)| *known == name)
        .map(|(_, shape)| *shape)
}

/// Represents who a command may be run as.
#[derive(Debug, Clone)]
pub struct RunasSpec {
    pub users: Vec<String>,
    pub groups: Vec<String>,
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
pub struct CmndSpec {
    /// Whether NOPASSWD is set for this command.
    pub nopasswd: bool,
    /// Whether NOEXEC is set for this command.
    pub noexec: bool,
    /// Whether SETENV is allowed.
    pub setenv: bool,
    /// The command pattern (path or ALL).
    pub command: String,
    /// Optional arguments pattern (empty = any args).
    pub args: String,
}

/// A complete privilege specification line.
#[derive(Debug, Clone)]
pub struct PrivilegeSpec {
    /// The user or group this applies to (may be an alias name, %group, etc.)
    pub users: Vec<String>,
    /// Hosts this applies on.
    pub hosts: Vec<String>,
    /// Runas specification.
    pub runas: RunasSpec,
    /// Allowed commands.
    pub commands: Vec<CmndSpec>,
}

/// Complete parsed sudoers configuration.
#[derive(Debug, Clone)]
pub struct SudoersConfig {
    pub user_aliases: HashMap<String, Vec<String>>,
    pub host_aliases: HashMap<String, Vec<String>>,
    pub cmnd_aliases: HashMap<String, Vec<String>>,
    pub runas_aliases: HashMap<String, Vec<String>>,
    pub defaults: Vec<DefaultsDirective>,
    pub privileges: Vec<PrivilegeSpec>,
}

impl Default for SudoersConfig {
    fn default() -> Self {
        Self::new()
    }
}

impl SudoersConfig {
    pub fn new() -> Self {
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
    pub fn get_default(&self, key: &str) -> Option<&str> {
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
    pub fn is_default_set(&self, key: &str) -> bool {
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
    pub fn env_keep_list(&self) -> Vec<String> {
        self.resolve_list("env_keep", DEFAULT_ENV_KEEP)
    }

    /// Get env_check list from Defaults.
    pub fn env_check_list(&self) -> Vec<String> {
        self.resolve_list("env_check", &[])
    }

    /// Get the timestamp_timeout (in seconds).
    pub fn timestamp_timeout(&self) -> u64 {
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
pub fn parse_sudoers(content: &str) -> Result<SudoersConfig, SudoError> {
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
                "Defaults setting {} is both negated and given a value",
                quoteaf_os(inner.trim())
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
                    "unterminated quote in Defaults value for {}",
                    quoteaf_os(name)
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
                "Defaults setting {} {reason}",
                quoteaf_os(name)
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
pub fn split_at_eq_outside_parens(s: &str) -> Option<(&str, &str)> {
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
pub fn parse_runas_prefix(s: &str) -> (RunasSpec, &str) {
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
pub enum CmndTag {
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

        // `!` negates the command after it, and sudoers allows white space
        // between the two: `! /usr/bin/passwd` is `!/usr/bin/passwd`. Folded
        // into the command here, so the split below sees the command rather
        // than a bare `!` whose "arguments" are the command.
        let (negated, rest) = strip_negation(part);
        // The command, then its arguments after the first white space.
        let (cmd, args) = split_command(rest);

        commands.push(CmndSpec {
            nopasswd,
            noexec,
            setenv,
            command: if negated {
                format!("!{cmd}")
            } else {
                cmd.to_string()
            },
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
pub struct SyntaxError {
    pub line_num: usize,
    pub message: String,
    pub is_warning: bool,
}

impl fmt::Display for SyntaxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let severity = if self.is_warning { "warning" } else { "error" };
        write!(f, "line {}: {}: {}", self.line_num, severity, self.message)
    }
}

/// Validate sudoers file content, returning any syntax errors.
pub fn validate_sudoers(content: &str, strict: bool) -> Vec<SyntaxError> {
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
                    message: format!("unknown Defaults setting {}", quoteaf_os(&setting.name)),
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
                        "Defaults setting {} is recognised but not yet honoured by this sudo",
                        quoteaf_os(&setting.name)
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
// The editor
// ============================================================================

/// The editor to launch, in sudo's documented order of preference.
///
/// `var_os`, not `var`: an editor setting names a program, and a program path
/// on this system may hold any byte but `/` and NUL. `var` reports a non-UTF-8
/// value as `Err(NotUnicode)`, which the `or_else` chain cannot distinguish
/// from "unset" — so `EDITOR=/opt/\xffed` silently fell through to the *default*
/// editor. Discarding the user's choice without a word is worse than either
/// honouring it or refusing it, and honouring it costs nothing.
///
/// Shared by `sudoedit` and `visudo`, which had a copy each. Two copies of a
/// preference order is a way to end up with two preference orders.
pub fn editor_command() -> OsString {
    env::var_os("SUDO_EDITOR")
        .or_else(|| env::var_os("VISUAL"))
        .or_else(|| env::var_os("EDITOR"))
        .unwrap_or_else(|| OsString::from(DEFAULT_EDITOR))
}

/// The editor setting split into words, as upstream splits it: `EDITOR="vim
/// -n"` runs `vim` with `-n`. An empty setting is the default editor.
// Called only by the unix half; the host build keeps it for its tests.
#[cfg_attr(not(unix), allow(dead_code))]
pub fn editor_words(editor: &OsStr) -> Vec<OsString> {
    let bytes = os_bytes(editor);
    let words: Vec<OsString> = bytes
        .split(u8::is_ascii_whitespace)
        .filter(|w| !w.is_empty())
        .map(os_from_bytes)
        .collect();
    if words.is_empty() {
        vec![OsString::from(DEFAULT_EDITOR)]
    } else {
        words
    }
}

/// `open(2)`'s `O_NOFOLLOW`: refuse a symlink in the last component. Linux's
/// value, which SlateOS's C library shares (a test holds the two together).
#[cfg(unix)]
pub const O_NOFOLLOW: i32 = 0o400_000;

// ============================================================================
// Command specs
// ============================================================================

/// A spec's `!`s, and what they negate. sudoers allows white space after
/// each, and two cancel.
pub fn strip_negation(mut spec: &str) -> (bool, &str) {
    let mut negated = false;
    while let Some(rest) = spec.strip_prefix('!') {
        negated = !negated;
        spec = rest.trim_start();
    }
    (negated, spec)
}

/// A command spec's command and its arguments, split at the first white
/// space; the arguments are empty when there are none.
pub fn split_command(spec: &str) -> (&str, &str) {
    let spec = spec.trim();
    match spec.find(char::is_whitespace) {
        Some(i) => (
            spec.get(..i).unwrap_or(spec),
            spec.get(i..).unwrap_or_default().trim(),
        ),
        None => (spec, ""),
    }
}
