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

use quoting::{escape_os, os_bytes, os_from_bytes, quoteaf_os};
use std::collections::HashMap;
use std::env;
use std::ffi::{OsStr, OsString};
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

/// Which of the four programs this invocation is, from `argv[0]`.
///
/// Takes bytes rather than `&str` because `argv[0]` is a path and a path here
/// may hold any byte but `/` and NUL. The four names it recognises are ASCII,
/// so a byte comparison decides exactly the same set as a string comparison
/// did -- what changes is that a name it does *not* recognise is now answered
/// (`Personality::Sudo`, the default) instead of aborting the process.
///
/// Generic over `AsRef<OsStr>` rather than taking `&OsStr` outright so that a
/// caller with a `&str` -- every test below, and any future one -- says
/// `detect_personality("visudo")` and not `detect_personality(OsStr::new(...))`.
/// The ceremony is what stops cases being added.
fn detect_personality<S: AsRef<OsStr>>(argv0: S) -> Personality {
    let argv0 = os_bytes(argv0.as_ref());
    // `rsplit` over both separators rather than a hand-rolled scan producing a
    // byte index to slice at: that index landed on a character boundary only
    // because `/` and `\` happen to be ASCII, which is a fact about the
    // separators rather than anything the code established. `rsplit` always
    // yields at least one item, so the fallback is unreachable.
    let base = argv0
        .rsplit(|&b| b == b'/' || b == b'\\')
        .next()
        .unwrap_or(&argv0);
    let base = base.strip_suffix(b".exe").unwrap_or(base);

    match base {
        b"sudoedit" => Personality::Sudoedit,
        b"visudo" => Personality::Visudo,
        b"sudoreplay" => Personality::Sudoreplay,
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
// Authorization checking
// ============================================================================

/// The command a decision is about, as sudoers sees it: upstream's
/// `ctx->user.cmnd` and `ctx->user.cmnd_args`.
///
/// `command` is the program as the caller named it -- or the pseudo-command
/// `sudoedit` -- and bytes, because it is a path the caller is about to `exec`
/// and a path may hold any byte but `/` and NUL. `args` is every argument after
/// it joined with single spaces, or `None` when there were none: what a rule's
/// `""` asks for, and not the same as one argument that is empty.
#[derive(Debug, Clone, Copy)]
struct Request<'a> {
    command: &'a [u8],
    args: Option<&'a [u8]>,
}

// Only the tests ask about a command without arguments by name.
#[cfg(test)]
impl<'a> Request<'a> {
    /// A request with no arguments.
    const fn bare(command: &'a [u8]) -> Self {
        Request {
            command,
            args: None,
        }
    }
}

/// sudo's `user_args`: the arguments joined with single spaces, or `None`
/// when there are none.
fn user_args(args: &[OsString]) -> Option<Vec<u8>> {
    if args.is_empty() {
        return None;
    }
    let mut joined = Vec::new();
    for (i, arg) in args.iter().enumerate() {
        if i > 0 {
            joined.push(b' ');
        }
        joined.extend_from_slice(&os_bytes(arg));
    }
    Some(joined)
}

/// What one command spec says about a request: sudoers' `ALLOW`, `DENY` or
/// `UNSPEC`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Verdict {
    /// The spec names the command, and grants it.
    Allow,
    /// The spec names the command, negated: it is refused, whatever an
    /// earlier rule said.
    Deny,
    /// The spec does not name the command; it decides nothing.
    Unspec,
}

impl Verdict {
    /// The verdict under a `!`.
    const fn negated(self) -> Self {
        match self {
            Verdict::Allow => Verdict::Deny,
            Verdict::Deny => Verdict::Allow,
            Verdict::Unspec => Verdict::Unspec,
        }
    }
}

/// How deep `Cmnd_Alias`es may name one another before one is taken to name
/// itself. Upstream refuses a loop when it reads the file; this matches
/// nothing instead, so a loop that got past the parser grants nothing.
const MAX_ALIAS_DEPTH: usize = 32;

/// Check whether the sudoers configuration lets `username` run `request` as
/// `target_user` (and `target_group`) on `hostname`, returning the command
/// spec that allowed it.
///
/// # sudoers' rule, which this follows
///
/// Privileges are read from the last to the first, and within one its
/// commands from the last to the first; **the first command spec that names
/// the request decides, either way** -- `!/usr/bin/passwd` refuses, and that
/// refusal stands over any earlier `ALL`. A spec that does not name the
/// request decides nothing. (`sudoers_lookup` and `cmndlist_matches`.)
///
/// Until 2026-10-01 a negated spec was read as "matches every command but this
/// one": `alice ALL = !/usr/bin/passwd` granted alice every other command, and
/// `alice ALL = ALL, !/usr/bin/passwd` granted passwd too, because the
/// negation did not match it and the search went on to `ALL`. And a rule's
/// arguments were never compared at all -- `alice ALL = /usr/bin/systemctl
/// restart nginx` let alice run `systemctl` with anything.
fn check_authorization(
    config: &SudoersConfig,
    username: &str,
    hostname: &str,
    target_user: &str,
    target_group: &str,
    request: &Request<'_>,
    user_groups: &[String],
) -> Option<CmndSpec> {
    // One secure path for every command spec in this decision, read once from
    // the configuration rather than per match.
    let secure = secure_path_of(config);
    let dirs = path_dirs(&secure);
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
            match cmnd_matches(
                &cmnd.command,
                &cmnd.args,
                request,
                &config.cmnd_aliases,
                &dirs,
                0,
            ) {
                Verdict::Allow => return Some(cmnd.clone()),
                Verdict::Deny => return None,
                Verdict::Unspec => {}
            }
        }
    }
    None
}

/// What one command spec -- a command, a `Cmnd_Alias`, either maybe negated
/// -- says about `request`: sudoers' `cmnd_matches`.
fn cmnd_matches(
    spec_cmd: &str,
    spec_args: &str,
    request: &Request<'_>,
    aliases: &HashMap<String, Vec<String>>,
    dirs: &[&str],
    depth: usize,
) -> Verdict {
    let (negated, spec) = strip_negation(spec_cmd);
    let verdict = if let Some(members) = aliases.get(spec) {
        // An alias is a list of its own, decided the same way: the last member
        // that names the request.
        if depth >= MAX_ALIAS_DEPTH {
            Verdict::Unspec
        } else {
            members
                .iter()
                .rev()
                .map(|member| {
                    let (cmd, args) = split_command(member);
                    cmnd_matches(cmd, args, request, aliases, dirs, depth.saturating_add(1))
                })
                .find(|v| *v != Verdict::Unspec)
                .unwrap_or(Verdict::Unspec)
        }
    } else if command_matches(spec, spec_args, request, dirs) {
        Verdict::Allow
    } else {
        Verdict::Unspec
    };
    if negated { verdict.negated() } else { verdict }
}

/// A spec's `!`s, and what they negate. sudoers allows white space after
/// each, and two cancel.
fn strip_negation(mut spec: &str) -> (bool, &str) {
    let mut negated = false;
    while let Some(rest) = spec.strip_prefix('!') {
        negated = !negated;
        spec = rest.trim_start();
    }
    (negated, spec)
}

/// A command spec's command and its arguments, split at the first white
/// space; the arguments are empty when there are none.
fn split_command(spec: &str) -> (&str, &str) {
    let spec = spec.trim();
    match spec.find(char::is_whitespace) {
        Some(i) => (
            spec.get(..i).unwrap_or(spec),
            spec.get(i..).unwrap_or_default().trim(),
        ),
        None => (spec, ""),
    }
}

/// Whether the caller's arguments satisfy a spec's: sudoers'
/// `command_args_match`.
///
/// A rule with no arguments allows any; `""` allows none; `^...$` is a POSIX
/// extended regular expression over the joined arguments; anything else is an
/// `fnmatch(3)` pattern over them -- with `FNM_PATHNAME` for `sudoedit`, whose
/// arguments are paths, so a `*` there never crosses a `/`.
fn args_match(spec_cmd: &str, spec_args: &str, user_args: Option<&[u8]>) -> bool {
    if spec_args.is_empty() {
        return true;
    }
    if spec_args == "\"\"" {
        return user_args.is_none();
    }
    let args = user_args.unwrap_or_default();
    if spec_args.len() > 1 && spec_args.starts_with('^') && spec_args.ends_with('$') {
        // A pattern that does not compile matches nothing, as upstream's
        // `regex_matches` denies: a rule that cannot be read grants nothing.
        return ere::Regex::new(spec_args.as_bytes())
            .is_ok_and(|re| re.is_match(args).unwrap_or(false));
    }
    let flags = if spec_cmd == "sudoedit" {
        fnmatch::Flags::PATHNAME
    } else {
        fnmatch::Flags::NONE
    };
    fnmatch::fnmatch(spec_args.as_bytes(), args, flags)
}

/// Check if a username matches a user specification list.
/// Evaluate a sudoers list in which **the last matching entry decides**.
///
/// # Why not the first
///
/// These lists are written to carve exceptions:
///
/// ```text
/// alice  ALL, !secret = (ALL) ALL
/// ```
///
/// Every one of them walked FORWARD and returned at the first match, so `ALL`
/// answered before `!secret` was ever looked at and the rule applied on
/// `secret` -- the one host the administrator had written it down to exclude.
/// `runas_matches` used `.any()` and did not read `!` at all.
///
/// Real sudo resolves a list by the last match, which is why the idiom works
/// there. This file already knew the rule and applied it one level up:
/// `check_authorization` iterates privileges `.rev()` with the comment "last
/// match wins, like real sudo". The lists inside them did the opposite.
///
/// Negation is handled here rather than in each caller's predicate, so a
/// matcher cannot forget it -- which is how `runas_matches` came not to have
/// it.
fn list_matches<F>(specs: &[String], hit: F) -> bool
where
    F: Fn(&str) -> bool,
{
    let mut verdict = false;
    for spec in specs {
        let (negated, name) = spec
            .strip_prefix('!')
            .map_or((false, spec.as_str()), |rest| (true, rest));
        if hit(name) {
            verdict = !negated;
        }
    }
    verdict
}

/// Does `name` name `target` directly, or through an alias that does?
fn names_or_aliases(name: &str, target: &str, aliases: &HashMap<String, Vec<String>>) -> bool {
    name == "ALL"
        || name == target
        || aliases
            .get(name)
            .is_some_and(|members| members.iter().any(|m| m == target || m == "ALL"))
}

fn user_matches(
    specs: &[String],
    username: &str,
    user_groups: &[String],
    aliases: &HashMap<String, Vec<String>>,
) -> bool {
    let in_group = |member: &str| {
        member
            .strip_prefix('%')
            .is_some_and(|g| user_groups.iter().any(|have| have == g))
    };
    list_matches(specs, |name| {
        names_or_aliases(name, username, aliases)
            || in_group(name)
            || aliases
                .get(name)
                .is_some_and(|members| members.iter().any(|m| in_group(m)))
    })
}

/// Check if a hostname matches a host specification list.
fn host_matches(specs: &[String], hostname: &str, aliases: &HashMap<String, Vec<String>>) -> bool {
    list_matches(specs, |name| names_or_aliases(name, hostname, aliases))
}

/// Check if target user/group matches a runas specification.
fn runas_matches(
    runas: &RunasSpec,
    target_user: &str,
    target_group: &str,
    aliases: &HashMap<String, Vec<String>>,
) -> bool {
    // `list_matches`, not `.any()`: this read `!` as part of a name, so a
    // `(ALL, !root)` runas spec let the caller run things AS root -- the one
    // target it was written to forbid.
    let user_ok = list_matches(&runas.users, |name| {
        names_or_aliases(name, target_user, aliases)
    });

    // If no group constraint specified, only check user.
    if target_group.is_empty() || runas.groups.is_empty() {
        return user_ok;
    }

    let group_ok = list_matches(&runas.groups, |name| {
        names_or_aliases(name, target_group, aliases)
    });

    user_ok && group_ok
}

/// Whether one command spec names `request`: sudoers' `command_matches`.
fn command_matches(spec: &str, spec_args: &str, request: &Request<'_>, dirs: &[&str]) -> bool {
    // `ALL` names every command, `sudoedit` among them, whatever its
    // arguments.
    if spec == "ALL" {
        return true;
    }
    // `sudoedit` is a pseudo-command: it names a request to edit and nothing
    // else, and its arguments are the files. A rule letting a user RUN
    // `/etc/motd` does not let them edit it, and a `sudoedit` rule does not
    // let them run a program called `sudoedit`.
    if spec == "sudoedit" || request.command == b"sudoedit" {
        return spec == "sudoedit"
            && request.command == b"sudoedit"
            && args_match(spec, spec_args, request.args);
    }
    command_path_matches(spec, request.command, dirs) && args_match(spec, spec_args, request.args)
}

/// Whether a command spec names the caller's program: the path half of
/// sudoers' `command_matches`.
///
/// Both are compared as whole paths, and the caller's is resolved first: on
/// the secure path when it names no directory, then made canonical, so
/// `/usr/bin/../../tmp/evil` is `/tmp/evil` before any rule looks at it and a
/// symlink is the file it points to. Then:
///
/// * a spec with a glob character (`*`, `?`, `[`) matches as sudoers'
///   `fnmatch` with `FNM_PATHNAME` does -- a `*` never crosses a `/`, so
///   `/usr/bin/*` is the programs in `/usr/bin` and nothing below or beside
///   it;
/// * a spec ending in `/` names every program directly in that directory;
/// * any other spec names one program: the same file, compared canonically
///   when both exist and by name when either does not, which is upstream's
///   own fallback.
///
/// Until 2026-10-01 a pattern was a prefix test on the path as typed, so
/// `alice ALL = /usr/bin/*` authorised `sudo /usr/bin/../../tmp/evil`.
///
/// `spec` is text -- it came out of `/etc/sudoers`. `actual` is bytes, for the
/// reason [`Request`] gives, so a program whose path is not text can match
/// only a pattern or a rule naming the same file.
fn command_path_matches(spec: &str, actual: &[u8], dirs: &[&str]) -> bool {
    let resolved = resolve_for_match(actual, dirs);
    let canonical = canonical_path(&resolved);
    if has_glob_meta(spec) {
        // A program that does not exist cannot run, so matching it by name
        // grants nothing -- but only a path with no `.`, `..` or empty
        // component may be matched that way, or the canonical form would be
        // what decides after all.
        let subject = canonical
            .as_deref()
            .or_else(|| is_clean_absolute(&resolved).then_some(resolved.as_slice()));
        return subject
            .is_some_and(|path| fnmatch::fnmatch(spec.as_bytes(), path, fnmatch::Flags::PATHNAME));
    }
    if spec.len() > 1
        && let Some(dir) = spec.strip_suffix('/')
    {
        // Every program directly in `dir`: the canonical program's parent
        // is the canonical directory.
        let want = canonical_path(dir.as_bytes()).unwrap_or_else(|| dir.as_bytes().to_vec());
        return canonical
            .as_deref()
            .and_then(parent_of)
            .is_some_and(|parent| parent == want);
    }
    // AN UNQUALIFIED SPEC IS RESOLVED, NOT BASENAME-MATCHED.
    //
    // This arm used to compare the last path component alone, and `actual` is
    // the caller's argv verbatim -- so `alice ALL = pkg` authorised
    //
    //     sudo /tmp/evil/pkg
    //
    // because `pkg == pkg`. The caller chooses that path and the binary runs
    // as root. Real sudoers requires fully-qualified commands for exactly this
    // reason; ours accepted a bare one as a convenience and gave away the
    // guarantee with it.
    //
    // Both sides are now resolved against the secure path and compared whole,
    // so `pkg` means the `pkg` that is actually on it -- and nothing else. A
    // spec naming a program that is not there matches nothing, which is the
    // safe direction: an unresolvable rule authorises nothing rather than
    // authorising by name.
    if !spec.contains('/') {
        let Some(spec_full) = first_on_secure_path(spec, dirs) else {
            return false;
        };
        return same_program(spec_full.as_bytes(), &resolved, canonical.as_deref());
    }
    same_program(spec.as_bytes(), &resolved, canonical.as_deref())
}

/// Whether a rule's path and the caller's resolved program are the same
/// program: the same canonical path when both exist, the same name when
/// either does not.
fn same_program(spec: &[u8], resolved: &[u8], canonical: Option<&[u8]>) -> bool {
    match (canonical_path(spec), canonical) {
        (Some(want), Some(have)) => want == have,
        _ => spec == resolved,
    }
}

/// `path` with every symlink, `.` and `..` resolved, as bytes; `None` when it
/// does not exist or cannot be resolved.
fn canonical_path(path: &[u8]) -> Option<Vec<u8>> {
    let resolved = fs::canonicalize(Path::new(&os_from_bytes(path))).ok()?;
    Some(os_bytes(resolved.as_os_str()).into_owned())
}

/// Whether `path` is absolute and has no `.`, `..` or empty component: a path
/// whose text is already its canonical form, symlinks aside.
fn is_clean_absolute(path: &[u8]) -> bool {
    let Some(rest) = path.strip_prefix(b"/") else {
        return false;
    };
    rest.split(|&b| b == b'/')
        .all(|part| !part.is_empty() && part != b"." && part != b"..")
}

/// Everything before the last `/` of an absolute path: `/` for `/x`.
fn parent_of(path: &[u8]) -> Option<&[u8]> {
    match path.iter().rposition(|&b| b == b'/') {
        Some(0) => Some(b"/"),
        Some(i) => path.get(..i),
        None => None,
    }
}

/// Whether a command spec is a pattern: sudoers' `has_meta`.
fn has_glob_meta(spec: &str) -> bool {
    spec.contains(['*', '?', '['])
}

/// The first executable named `command` in `dirs`, or `None`.
///
/// Splitting the search from the `PATH` string is deliberate: an absolute path
/// on the development host begins with a drive letter and a colon, so a test
/// handing a real directory to something that splits on `:` searches two
/// fragments of it and fails in a way that reads like the code being wrong.
fn first_on_secure_path(command: &str, dirs: &[&str]) -> Option<String> {
    dirs.iter()
        .map(|dir| format!("{dir}/{command}"))
        .find(|candidate| fs::metadata(candidate).is_ok())
}

/// `name` as a whole path: unchanged if it already has one, else resolved
/// against `dirs`.
///
/// A name that is not valid UTF-8 is returned unchanged. It can then only
/// match a spec it equals byte for byte, which is correct -- a sudoers file is
/// text, so a command whose name is not text is named by no spec in it.
fn resolve_for_match(name: &[u8], dirs: &[&str]) -> Vec<u8> {
    if name.contains(&b'/') {
        return name.to_vec();
    }
    match core::str::from_utf8(name) {
        Ok(text) => {
            first_on_secure_path(text, dirs).map_or_else(|| name.to_vec(), String::into_bytes)
        }
        Err(_) => name.to_vec(),
    }
}

/// The `PATH` an unqualified command resolves against, from `Defaults
/// secure_path` when the administrator set one.
///
/// `secure_path` was parsed and stored and **never read** until 2026-09-12 --
/// a setting that accepted a value and did nothing with it. This is its first
/// consumer, and the default matches what `sudo` ships.
fn secure_path_of(config: &SudoersConfig) -> String {
    config.get_default("secure_path").map_or_else(
        || "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin".to_string(),
        ToString::to_string,
    )
}

/// The directories of a `PATH`, in order, with empty entries dropped.
fn path_dirs(path: &str) -> Vec<&str> {
    path.split(':').filter(|d| !d.is_empty()).collect()
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

/// Is there a valid cached credential for `username`?
///
/// # Every failure answers `false`, and that is the safe direction
///
/// `false` means "ask for the password". So an unreadable timestamp file, an
/// unparsable one, and a missing one all lead to a prompt, which is the
/// outcome that cannot let anybody through. This is the opposite direction
/// from `mkfs`/`fsck`'s old `is_mounted`, and for the opposite reason: there,
/// `false` meant "safe to write" and a failed read waved a destructive
/// operation through; here `false` costs the user one password entry.
///
/// **Do not "improve" this by returning `true` when the file cannot be read.**
/// That would hand out a cached authentication on the strength of a failed
/// read, which is the whole thing a credential cache must not do.
///
/// # The error arms were never the problem
///
/// The paragraph above was written on 2026-09-10 during a sweep for predicates
/// that answer `false` on failure, and it concluded "this one is correct as
/// written", noted "so the next sweep does not have to re-derive it". It was
/// right about every arm it looked at and the defect was in the other one:
///
/// ```text
/// now.saturating_sub(ts) < timeout
/// ```
///
/// With `ts` **in the future**, `saturating_sub` yields 0, `0 < timeout` is
/// true, and the password prompt is skipped -- for as long as the clock takes
/// to catch up. No failure occurs anywhere; the read succeeds, the parse
/// succeeds, and the arithmetic answers a question nobody asked.
///
/// A backwards clock step is ordinary: an NTP correction, an RTC read at boot
/// before the network is up, a restored VM snapshot. Each turns a legitimately
/// written timestamp into a future one and the cache into a permanent one.
/// Real `sudo` treats this as `TS_FATAL` -- "timestamp too far in the future".
///
/// So the rule this function's own heading states -- every failure answers
/// `false` -- is kept, and a *success* that cannot mean what it says is now
/// one of the things that answers `false` too.
fn check_timestamp(username: &str, timeout: u64) -> bool {
    match fs::read_to_string(timestamp_path(username)) {
        Ok(content) => timestamp_is_fresh(&content, current_epoch(), timeout),
        Err(_) => false,
    }
}

/// Whether the timestamp file's `content` records an authentication that is
/// still current at `now`.
///
/// Split out from [`check_timestamp`] so the clock is a parameter: the
/// interesting cases are a timestamp in the future and one exactly at the
/// timeout boundary, and neither can be reached by a test that has to use the
/// real clock and the real file.
fn timestamp_is_fresh(content: &str, now: u64, timeout: u64) -> bool {
    let Some(ts_str) = content.lines().next() else {
        return false;
    };
    let Ok(ts) = ts_str.trim().parse::<u64>() else {
        return false;
    };
    // Checked BEFORE the never-expires arm. `timestamp_timeout=-1` is a
    // statement about how long an authentication lasts, not a licence to trust
    // a file whose contents cannot be true.
    if ts > now {
        return false;
    }
    // `invalidate_timestamp` writes 0 "to invalidate without removing", and 0
    // is 1970 -- comfortably expired under every finite timeout, and NOT
    // expired under `timestamp_timeout=-1`, where nothing expires. So `sudo -k`
    // was a no-op for exactly the configuration that most needs it to work.
    // The sentinel is honoured by the READER, because the reader is what has
    // to agree with it.
    if ts == 0 {
        return false;
    }
    if timeout == u64::MAX {
        return true;
    }
    now.saturating_sub(ts) < timeout
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
///
/// Reads the environment with `vars_os`, not `vars`. `std::env::vars()`'s
/// iterator panics on a variable whose name *or value* is not valid Unicode,
/// and `PATH` on this OS is a list of paths, which may hold any byte but `/`
/// and NUL. So one stray byte anywhere in the inherited environment made this
/// program unable to run anything at all — including the `-E`-less default
/// path, which was about to *discard* that variable.
///
/// The keys the sudoers lists and `ENV_BLACKLIST` name are ASCII, so comparing
/// them against `OsStr` decides the same set as before; what changes is that a
/// variable outside those sets is now dropped or carried rather than fatal.
fn build_environment(
    config: &SudoersConfig,
    preserve_env: bool,
    target_user: &str,
    target_home: &str,
    target_shell: &str,
    login_shell: bool,
) -> Vec<(OsString, OsString)> {
    let env_reset = config.is_default_set("env_reset") || config.get_default("env_reset").is_none();
    let keep_list = config.env_keep_list();
    let check_list = config.env_check_list();

    let blacklisted = |key: &OsStr| ENV_BLACKLIST.iter().any(|&b| key == OsStr::new(b));

    let mut env: Vec<(OsString, OsString)> = Vec::new();

    if preserve_env {
        // -E flag: preserve all current env vars except blacklisted.
        for (key, val) in std::env::vars_os() {
            if !blacklisted(&key) {
                env.push((key, val));
            }
        }
    } else if env_reset {
        // Default: reset environment, only keep allowed vars.
        for (key, val) in std::env::vars_os() {
            if keep_list.iter().any(|k| OsStr::new(k) == key) {
                // Check for dangerous values in env_check vars. Bytewise: `/`
                // and `%` are single bytes in UTF-8 and can be no part of a
                // multi-byte character, so scanning for them in the raw value
                // finds exactly what scanning the decoded string found — and
                // it also works on the values that could not be decoded, which
                // are precisely the ones a caller would try to smuggle through.
                let suspicious = check_list.iter().any(|k| OsStr::new(k) == key)
                    && os_bytes(&val).iter().any(|&b| b == b'/' || b == b'%');
                if suspicious {
                    continue; // Skip suspicious values.
                }
                if !blacklisted(&key) {
                    env.push((key, val));
                }
            }
        }
    } else {
        // No env_reset: inherit everything except blacklisted.
        for (key, val) in std::env::vars_os() {
            if !blacklisted(&key) {
                env.push((key, val));
            }
        }
    }

    // Always set these.
    set_or_replace(&mut env, "USER", target_user.as_ref());
    set_or_replace(&mut env, "LOGNAME", target_user.as_ref());
    // SUDO_USER is what the command being run sees as "who invoked me", so it
    // is the same identity claim as the one the sudoers match is made on and
    // must come from the same place. It read `$USER` too, so a forged name
    // propagated into the child's environment as sudo's own attestation.
    set_or_replace(&mut env, "SUDO_USER", require_username().as_ref());

    if login_shell {
        set_or_replace(&mut env, "HOME", target_home.as_ref());
        set_or_replace(&mut env, "SHELL", target_shell.as_ref());
        set_or_replace(
            &mut env,
            "PATH",
            "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin".as_ref(),
        );
    } else {
        // Preserve HOME and SHELL from current env or set to target.
        if !env.iter().any(|(k, _)| k == "HOME") {
            env.push((OsString::from("HOME"), OsString::from(target_home)));
        }
        if !env.iter().any(|(k, _)| k == "SHELL") {
            env.push((OsString::from("SHELL"), OsString::from(target_shell)));
        }
    }

    // Record original command info.
    if let Ok(pwd) = std::env::current_dir() {
        set_or_replace(&mut env, "SUDO_COMMAND", "".as_ref());
        set_or_replace(&mut env, "SUDO_GID", format!("{}", current_gid()).as_ref());
        set_or_replace(&mut env, "SUDO_UID", format!("{}", current_uid()).as_ref());
        let _ = pwd; // Acknowledged: we set SUDO_COMMAND to empty initially.
    }

    env
}

/// Set or replace an environment variable in the env list.
///
/// `key` stays `&str` because every caller passes a literal: these are the
/// names sudo itself sets, not names it received from anywhere.
fn set_or_replace(env: &mut Vec<(OsString, OsString)>, key: &str, val: &OsStr) {
    if let Some(entry) = env.iter_mut().find(|(k, _)| k == key) {
        entry.1 = val.to_os_string();
    } else {
        env.push((OsString::from(key), val.to_os_string()));
    }
}

// ============================================================================
// Logging
// ============================================================================

/// Log a sudo command execution.
///
/// **Every variable field goes through [`escape_os`], and that is not
/// cosmetic.** The log is one record per line with ` ; ` between fields, so any
/// field able to carry a newline could *append a line of its own*: anyone able
/// to run `sudo` at all could write a fabricated `RESULT=ALLOWED` entry naming
/// another user, into the very file whose purpose is to say who ran what.
/// Three fields could carry one, by three different routes:
///
/// - `command` — argv, chosen outright by the caller.
/// - `tty` — read from `$TTY`, which is just an environment variable the
///   caller sets; nothing validates it.
/// - `pwd` — the working directory, so a `mkdir` of a name containing a
///   newline and a `cd` into it is the whole exploit. Path names here may hold
///   every byte but `/` and NUL, so this is an ordinary directory, not a
///   malformed one.
///
/// `username` and `target_user` are escaped too. They come from the user
/// database rather than from argv, so they are a step further away, but "a step
/// further away" is not a property worth relying on in an audit log, and
/// escaping a name that was already plain leaves it unchanged.
///
/// [`escape_os`] renders `\n` as the two characters `\n`, and a byte that is
/// not part of a character as three octal digits, so the record stays one line,
/// stays readable, and says what actually happened.
///
/// The formatting lives in [`format_log_record`] so that the escaping above is
/// pinned by tests rather than only by this paragraph — the writing half needs
/// a root-owned `/var/log`, which no test has, and a security property that can
/// only be checked by reading the source is one that comes back.
fn log_command(
    username: &str,
    tty: &OsStr,
    pwd: &OsStr,
    target_user: &str,
    command: &OsStr,
    result: &str,
) {
    let log_line = format_log_record(
        &format_timestamp(current_epoch()),
        username,
        tty,
        pwd,
        target_user,
        command,
        result,
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

/// Render one audit record, including its trailing newline.
///
/// Split out of [`log_command`] purely so it can be tested; see that function
/// for why each field is escaped. Returns a `String` rather than bytes because
/// [`escape_os`]'s output is always valid UTF-8 by construction — it emits a
/// byte that is not part of a character as three octal digits — so the record
/// is text no matter what went into it.
fn format_log_record(
    timestamp: &str,
    username: &str,
    tty: &OsStr,
    pwd: &OsStr,
    target_user: &str,
    command: &OsStr,
    result: &str,
) -> String {
    let username = escape_os(username);
    let tty = escape_os(tty);
    let pwd = escape_os(pwd);
    let target_user = escape_os(target_user);
    let command = escape_os(command);
    format!(
        "{timestamp} : {username} : TTY={tty} ; PWD={pwd} ; USER={target_user} ; COMMAND={command} ; RESULT={result}\n"
    )
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
    /// The session's directory name. An `OsString` because that is what a
    /// directory name is: the previous `String` was filled from
    /// `file_name().and_then(|n| n.to_str())`, whose `None` arm `continue`d --
    /// so a session directory whose name is not valid UTF-8 did not fail to
    /// replay, it failed to *appear*, and `sudoreplay -l` listed the recording
    /// as though it had never been made.
    id: OsString,
    user: String,
    target_user: String,
    command: String,
    timestamp: u64,
    _tty: String,
}

/// List recorded sessions from the I/O log directory.
fn list_sessions(io_dir: &OsStr) -> Vec<SessionEntry> {
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

        let Some(session_id) = path.file_name().map(OsStr::to_os_string) else {
            continue;
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
fn replay_session(io_dir: &OsStr, session_id: &OsStr, speed_factor: f64) -> Result<(), SudoError> {
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
        eprintln!("Replaying session {}:", quoteaf_os(session_id));
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

/// The caller's login name, or `None` if this build cannot determine it.
///
/// # Why this may not read the environment
///
/// It used to be `$USER`, then `$LOGNAME`, then the literal `"unknown"`. All
/// three are the caller's to set, and this name decides three things:
///
///   * **which sudoers rules apply** -- `user_matches` compares it against
///     each privilege spec's user list;
///   * **whose password is demanded** -- `authenticate(&username, ..)`;
///   * **which credential cache is consulted** -- `timestamp_path` is
///     `TIMESTAMP_DIR/<username>`.
///
/// The third is the one that turns this from mis-attribution into privilege
/// escalation. `check_timestamp` skips the password prompt entirely when the
/// named user authenticated recently, so with the name under the caller's
/// control:
///
///     USER=alice sudo <command>
///
/// finds alice's live timestamp, asks for no password at all, and is then
/// authorised against alice's rules. No knowledge of alice's password is
/// needed -- only that she ran sudo in the last few minutes.
///
/// So the name comes from `getuid(2)` through `authlib::identity::caller_uid`
/// and is resolved against the user database, exactly as `effective_uid`
/// below already does for the id. That function's own note records this
/// lesson being learned once already: "This used to read /proc/self/status and
/// fall back to the UID environment variable. The fallback was the caller's to
/// set". The username was left behind, as the hostname was.
fn current_username() -> Option<String> {
    let uid = authlib::identity::caller_uid()?;
    userdb::UserDb::load(userdb::DEFAULT_PATH)
        .ok()
        .and_then(|db| db.find_uid(uid).and_then(userdb::Record::username))
}

/// The caller's login name, or a refusal that says why.
///
/// **No numeric fallback.** A uid with no account record could be spelled
/// `uid1000` and matched against sudoers, but a sudoers file naming `uid1000`
/// is not a thing anyone writes, so the match would fail and the denial would
/// look like a policy decision rather than a lookup failure. Worse, the same
/// string would key a timestamp file, giving every unnamed uid one shared
/// credential cache. Refusing names the real problem.
fn require_username() -> String {
    match current_username() {
        Some(u) => u,
        None => {
            eprintln!(
                "sudo: cannot determine who you are; refusing rather than                  guessing at a name that selects your privileges"
            );
            process::exit(1);
        }
    }
}

/// Get the current hostname.
/// The live hostname, or `None` if this build cannot determine it.
///
/// # Why this may not fall back to anything
///
/// The hostname SELECTS WHICH SUDOERS RULES APPLY -- `host_matches` compares
/// it against each privilege spec's host list, so `alice web01=(ALL) ALL` and
/// `alice ALL=(ALL) ALL` are different grants and the hostname picks between
/// them.
///
/// This used to try `/etc/hostname` and then fall back to **`$HOSTNAME`**,
/// which the caller sets, and then to the literal `"localhost"`. So
/// `HOSTNAME=web01 sudo ...` chose its own sudoers rules. The function
/// directly below records the same lesson for the uid -- "This used to read
/// /proc/self/status and fall back to the UID environment variable. The
/// fallback was the caller's to set" -- and this one was left behind when
/// that was repaired.
///
/// `/etc/hostname` is wrong here for a second reason even without the
/// environment fallback: it is the PERSISTENT name, the one the live name is
/// set from at boot. A machine renamed since boot has two different answers
/// and the sudoers rules are about the live one. `posix`'s own
/// `current_hostname` says the same thing and reads only the kernel.
fn current_hostname() -> Option<String> {
    let name = fs::read_to_string("/proc/sys/kernel/hostname").ok()?;
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    Some(name.to_string())
}

/// The hostname, or a refusal that says why.
///
/// An unknown hostname cannot be resolved to "probably fine": it would mean
/// evaluating host-specific rules against a name nobody supplied. This exits
/// rather than guessing, for the same reason [`UNKNOWN_CALLER_ID`] is not
/// zero -- when the value decides whether a password is demanded, not knowing
/// it has to be the unprivileged answer.
fn require_hostname() -> String {
    match current_hostname() {
        Some(h) => h,
        None => {
            eprintln!(
                "sudo: cannot determine this host's name, and the sudoers rules                  that apply are chosen by it; refusing"
            );
            process::exit(1);
        }
    }
}

/// The id a caller this build cannot identify is treated as.
///
/// **Deliberately not zero.** These values decide whether a password is
/// demanded, so an unknown caller has to be an unprivileged one. Kept as a
/// named constant rather than a literal in two places because the *reason* is
/// the important part and a bare `1000` does not carry it.
const UNKNOWN_CALLER_ID: u32 = 1000;

/// The real uid of the calling process.
///
/// `getuid(2)`, via `authlib::identity::caller_uid`.
///
/// This used to read `/proc/self/status` and fall back to the `UID`
/// environment variable. The fallback was the caller's to set, and this value
/// decides whether a password is demanded -- so `UID=0 sudo <command>` on a
/// system with no readable procfs skipped the prompt. The `getuid` call has no
/// file to be missing and no input a parent process can supply.
fn current_uid() -> u32 {
    authlib::identity::caller_uid().unwrap_or(UNKNOWN_CALLER_ID)
}

/// The real gid of the calling process. See [`current_uid`].
fn current_gid() -> u32 {
    authlib::identity::caller_gid().unwrap_or(UNKNOWN_CALLER_ID)
}

/// Get the current tty name.
///
/// The terminal this command was run from, for the audit record.
///
/// # It used to be `$TTY`
///
///     env::var_os("TTY").unwrap_or_else(|| OsString::from("unknown"))
///
/// so the subject of the audit record chose what the record said about them.
/// Bounded -- the value is escaped where it is written, so it could not forge
/// whole log lines, and nothing branches on it -- which is why this was an
/// entry in `known-issues.md` rather than a same-day fix after the `$USER` and
/// `$HOSTNAME` repairs. It is still the family those two belonged to, and an
/// audit log is read precisely when somebody is working out what happened.
///
/// `ttyname(0)` is what real sudo uses. The argument the old comment made for
/// `var_os` over `var` survives the change and is why the bytes are not put
/// through UTF-8: a tty name is a path under `/dev`, and this OS allows any
/// byte but `/` and NUL in one.
#[cfg(unix)]
fn current_tty() -> OsString {
    unsafe extern "C" {
        fn ttyname(fd: i32) -> *const std::ffi::c_char;
    }
    // SAFETY: `ttyname` takes a descriptor and returns either null or a
    // pointer to a NUL-terminated string in static storage, valid until the
    // next call to it. The bytes are copied out before anything else runs.
    let bytes = unsafe {
        let ptr = ttyname(0);
        if ptr.is_null() {
            // Null is "descriptor 0 is not a terminal", which is a real
            // answer and a different one from "there is a terminal and I
            // could not name it". `none` rather than `unknown` says which.
            return OsString::from("none");
        }
        std::ffi::CStr::from_ptr(ptr).to_bytes().to_vec()
    };
    // Bytes all the way, which is the reason this returned `OsString` in the
    // first place: a tty name is a path under /dev and may hold any byte but
    // `/` and NUL.
    std::os::unix::ffi::OsStringExt::from_vec(bytes)
}

/// The host build has no `ttyname`, and inventing a terminal for an audit
/// record is the defect this function was just repaired for.
#[cfg(not(unix))]
fn current_tty() -> OsString {
    OsString::from("unknown")
}

/// The working directory, for the `PWD=` field of the audit log.
///
/// Extracted because the expression it replaces appeared at six call sites,
/// each spelling `current_dir().map(|p| p.display().to_string())` by hand — and
/// `display()` substitutes U+FFFD for any byte it cannot decode, so six copies
/// of the same lossy rendering had to be found and fixed together or not at
/// all. A directory name here may hold every byte but `/` and NUL, so the loss
/// is reachable from an ordinary `mkdir`. The value is escaped at the point it
/// is written; see [`log_command`].
fn current_pwd() -> OsString {
    env::current_dir().map_or_else(|_| OsString::from("unknown"), PathBuf::into_os_string)
}

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
fn editor_command() -> OsString {
    env::var_os("SUDO_EDITOR")
        .or_else(|| env::var_os("VISUAL"))
        .or_else(|| env::var_os("EDITOR"))
        .unwrap_or_else(|| OsString::from(DEFAULT_EDITOR))
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
/// What `sudo` needs to know about the user it is about to become.
struct TargetUser {
    home: String,
    shell: String,
    /// `(uid, gid)`, or `None` if the account database cannot name them.
    ///
    /// Kept beside `home` and `shell` rather than looked up separately on
    /// purpose. A second `UserDb::load` would be a second read of the same
    /// file, and the two could disagree -- the account could be edited between
    /// them -- which would mean running with one record's home directory under
    /// another record's uid. One read, one answer.
    ids: Option<(u32, u32)>,
}

/// Look the target user up once.
///
/// `home` and `shell` fall back to conventional defaults for an account the
/// database does not have, because a missing shell is survivable. `ids` does
/// not fall back: there is no safe default for "which user to run as", and
/// the caller refuses rather than inventing one.
fn get_user_info(username: &str) -> TargetUser {
    let default_home = if username == "root" {
        "/root".to_string()
    } else {
        format!("/home/{username}")
    };
    let default_shell = "/bin/sh".to_string();

    let Ok(db) = userdb::UserDb::load(userdb::DEFAULT_PATH) else {
        return TargetUser {
            home: default_home,
            shell: default_shell,
            ids: None,
        };
    };
    let Some(record) = db.find(username) else {
        return TargetUser {
            home: default_home,
            shell: default_shell,
            ids: None,
        };
    };

    TargetUser {
        home: record
            .home()
            .filter(|h| !h.is_empty())
            .unwrap_or(default_home),
        shell: record.shell().unwrap_or(default_shell),
        // No `gid:` of its own means the user-private group, which `useradd`
        // numbers after the uid.
        ids: record.uid().map(|uid| (uid, record.gid().unwrap_or(uid))),
    }
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
///
/// # Why the command is bytes and the names are not
///
/// `command` is [`OsString`] and everything else here is [`String`], and the
/// split is deliberate rather than half-finished work.
///
/// The command becomes a path and is handed to `exec`. A path on this OS may
/// hold every byte but `/` and NUL, so narrowing it to text would either panic
/// (what this crate did until 2026-09-06) or *change which program runs* --
/// a lossy conversion maps distinct byte strings onto one `U+FFFD`-bearing
/// name, and in the crate that grants root, two arguments that collapse into
/// one is the worst available failure.
///
/// The names -- `-u`, `-g` -- are matched against `/etc/sudoers` and
/// `/etc/users.yaml`, which are text files. A name that is not valid text
/// cannot equal any entry in them, so the only possible outcome is a denial.
/// [`parse_sudo_args`] therefore refuses such a value at the boundary, which
/// reaches that same outcome sooner and says why. `-p` is a prompt: it is only
/// ever printed, and printing it losslessly is [`escape_os`]'s job, not this
/// struct's.
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
    command: Vec<OsString>,
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
fn parse_sudo_args(args: &[OsString]) -> Result<SudoOpts, SudoError> {
    let mut opts = SudoOpts::default();
    let mut rest = args;

    while let Some((arg, tail)) = rest.split_first() {
        rest = tail;
        // Every option this program understands is ASCII, so the decisions
        // below are the same ones the `&str` version made -- but they are made
        // on bytes, because the *values* between the options are paths.
        let bytes = os_bytes(arg);

        if *bytes == *b"--" {
            // Everything after `--` is the command, options included.
            opts.command.extend(rest.iter().cloned());
            break;
        }

        if !bytes.starts_with(b"-") {
            // The first non-option argument starts the command, and takes the
            // remainder with it.
            opts.command.push(arg.clone());
            opts.command.extend(rest.iter().cloned());
            break;
        }

        if bytes.starts_with(b"--") {
            return Err(SudoError::UsageError(format!(
                "unknown option: {}",
                quoteaf_os(arg)
            )));
        }

        let flags = bytes.strip_prefix(b"-").unwrap_or(&bytes);
        if flags.is_empty() {
            // A bare `-`, which names no option at all.
            return Err(SudoError::UsageError(format!(
                "unknown option: {}",
                quoteaf_os(arg)
            )));
        }

        let mut bundle = flags;
        while let Some((&lead, glued)) = bundle.split_first() {
            bundle = glued;
            // Splitting off one *byte* is what makes `glued` correct for a
            // value that is not text at all -- `-u` never takes one, but `-p`
            // may, and the old `chars()` walk could not produce a remainder it
            // could not first decode. Every flag below is ASCII, so a byte is
            // also a whole character here; a non-ASCII lead byte is not a flag
            // and is refused before any remainder is computed, which is where
            // an `as_bytes()` index would have split a character in half.
            let Some(flag) = lead.is_ascii().then(|| char::from(lead)) else {
                return Err(SudoError::UsageError(format!(
                    "unknown option in {}",
                    quoteaf_os(arg)
                )));
            };
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
                        os_from_bytes(glued)
                    };
                    // Refused here rather than carried and lost later: `-u` and
                    // `-g` name entries in text files and `-p` is printed, so a
                    // value that is not text can only ever end in a denial or a
                    // mangled prompt. Saying so names the option; denying it
                    // three hundred lines later would not. See `SudoOpts`.
                    let value = value.into_string().map_err(|bad| {
                        SudoError::UsageError(format!(
                            "-{flag}: argument is not valid text: {}",
                            quoteaf_os(&bad)
                        ))
                    })?;
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
///
/// `file` is [`OsString`]: `-f` names a file to open, and an alternate sudoers
/// path is exactly as free in its bytes as any other path on this OS.
#[derive(Debug)]
struct VisudoOpts {
    check_only: bool,
    file: OsString,
    strict: bool,
}

impl Default for VisudoOpts {
    fn default() -> Self {
        Self {
            check_only: false,
            file: OsString::from(SUDOERS_PATH),
            strict: false,
        }
    }
}

/// Parse visudo command-line arguments.
fn parse_visudo_args(args: &[OsString]) -> Result<VisudoOpts, SudoError> {
    let mut opts = VisudoOpts::default();
    // A slice cursor, as in `parse_sudo_args`: `-f` takes its value from a tail
    // already proved non-empty, so there is no `i + 1` to bounds-check
    // separately from the `args[i + 1]` that follows it.
    let mut rest = args;

    while let Some((arg, tail)) = rest.split_first() {
        rest = tail;
        // Matching on bytes rather than on `&str`: the three options are ASCII
        // so the recognised set does not change, and an argument that is not
        // text now reaches the diagnostic below instead of the panic that used
        // to happen before this function was ever entered.
        match &*os_bytes(arg) {
            b"-c" => opts.check_only = true,
            b"-s" => opts.strict = true,
            b"-f" => {
                let Some((value, after_value)) = rest.split_first() else {
                    return Err(SudoError::UsageError("-f requires an argument".to_string()));
                };
                opts.file = value.clone();
                rest = after_value;
            }
            other if other.starts_with(b"-") => {
                return Err(SudoError::UsageError(format!(
                    "unknown option: {}",
                    quoteaf_os(arg)
                )));
            }
            _ => {
                return Err(SudoError::UsageError(format!(
                    "unexpected argument: {}",
                    quoteaf_os(arg)
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
///
/// `directory` and `session_id` are [`OsString`] because both are joined into
/// a path — the session id is a directory name under `directory`, not a label.
/// `speed_factor` stays an `f64`: it is a number, and a value that does not
/// parse as one was already an error before any of this.
#[derive(Debug)]
struct SudoreplayOpts {
    list: bool,
    directory: OsString,
    speed_factor: f64,
    session_id: Option<OsString>,
}

impl Default for SudoreplayOpts {
    fn default() -> Self {
        Self {
            list: false,
            directory: OsString::from(SUDO_IO_DIR),
            speed_factor: 1.0,
            session_id: None,
        }
    }
}

/// Parse sudoreplay command-line arguments.
fn parse_sudoreplay_args(args: &[OsString]) -> Result<SudoreplayOpts, SudoError> {
    let mut opts = SudoreplayOpts::default();
    // A slice cursor, as in `parse_sudo_args` and `parse_visudo_args`.
    let mut rest = args;

    while let Some((arg, tail)) = rest.split_first() {
        rest = tail;
        // On bytes, as in `parse_visudo_args`, and for the same reason.
        match &*os_bytes(arg) {
            b"-l" => opts.list = true,
            b"-d" => {
                let Some((value, after_value)) = rest.split_first() else {
                    return Err(SudoError::UsageError("-d requires an argument".to_string()));
                };
                opts.directory = value.clone();
                rest = after_value;
            }
            b"-s" => {
                let Some((value, after_value)) = rest.split_first() else {
                    return Err(SudoError::UsageError("-s requires an argument".to_string()));
                };
                rest = after_value;
                // A speed factor that is not text is not a number either, so it
                // takes the same arm as `-s wombat` rather than a second one.
                opts.speed_factor = value
                    .to_str()
                    .and_then(|v| v.parse::<f64>().ok())
                    .ok_or_else(|| SudoError::UsageError("invalid speed factor".to_string()))?;
                if opts.speed_factor <= 0.0 {
                    return Err(SudoError::UsageError(
                        "speed factor must be positive".to_string(),
                    ));
                }
            }
            other if other.starts_with(b"-") => {
                return Err(SudoError::UsageError(format!(
                    "unknown option: {}",
                    quoteaf_os(arg)
                )));
            }
            _ => {
                opts.session_id = Some(arg.clone());
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
fn run_sudo(args: &[OsString]) -> i32 {
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

    let username = require_username();
    let hostname = require_hostname();

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
                            &current_pwd(),
                            &opts.target_user,
                            OsStr::new("(validate)"),
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
    let target = get_user_info(&opts.target_user);
    let (target_home, target_shell) = (target.home.clone(), target.shell.clone());

    // What runs, and what sudoers is asked about -- the same thing, as upstream
    // arranges it. See `invocation`: with `-s` or `-i` it is the shell.
    let Some((program, program_args)) = invocation(&opts, &target_shell) else {
        eprintln!("sudo: no command to execute");
        print_sudo_usage();
        return 1;
    };

    // The whole command as one string, for the log and the messages: the
    // program and its arguments as they will run. Built by pushing rather than
    // by `join`, because `[OsString]` has no `join` and because joining the
    // *lossy* forms would record a different command from the one authorised.
    let command_str = {
        let mut joined = program.clone();
        for part in &program_args {
            joined.push(" ");
            joined.push(part);
        }
        joined
    };

    // Check authorization.
    let program_bytes = os_bytes(&program).into_owned();
    let joined_args = user_args(&program_args);
    let request = Request {
        command: &program_bytes,
        args: joined_args.as_deref(),
    };
    let auth_result = check_authorization(
        &config,
        &username,
        &hostname,
        &opts.target_user,
        &opts.target_group,
        &request,
        &user_groups,
    );

    let cmnd_spec = match auth_result {
        Some(spec) => spec,
        None => {
            eprintln!(
                "sudo: {username} is not allowed to run {} as {} on {hostname}",
                quoteaf_os(&command_str),
                quoteaf_os(&opts.target_user)
            );
            log_command(
                &username,
                &current_tty(),
                &current_pwd(),
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
                    &current_pwd(),
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
                            &current_pwd(),
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
        &current_pwd(),
        &opts.target_user,
        &command_str,
        "ALLOWED",
    );

    // Execute the program sudoers authorised: the same resolution
    // `command_path_matches` judged -- on the secure path when unqualified,
    // then canonical -- so the file that runs is the file that was allowed,
    // whatever `PATH` or a symlink says by the time the child starts. A child
    // process, which `become_user` below turns into the target user between
    // fork and exec; real sudo also runs the command as a child (it stays to
    // log and relay signals).
    let secure = secure_path_of(&config);
    let dirs = path_dirs(&secure);
    let resolved = resolve_for_match(&program_bytes, &dirs);
    let exec_path = os_from_bytes(&canonical_path(&resolved).unwrap_or(resolved));
    let mut cmd = process::Command::new(&exec_path);
    // The name the program sees is the one the caller gave -- and for `-i`,
    // the login form (`-sh`), which is how a shell is told it is a login
    // shell, as upstream tells it.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt as _;
        if opts.login_shell {
            cmd.arg0(authlib::identity::login_argv0(&program));
        } else {
            cmd.arg0(&program);
        }
    }
    cmd.args(&program_args);

    // Set the environment.
    cmd.env_clear();
    for (key, val) in &_env {
        cmd.env(key, val);
    }

    if opts.login_shell {
        cmd.current_dir(&target_home);
    }

    // Become the target user. Until now `sudo` authorised the command against
    // `/etc/sudoers` and then ran it as the caller: the environment named the
    // target and every credential the system checks named whoever typed
    // `sudo`. See `authlib::identity::become_user` for the ordering rule and
    // for the one part of the drop that is still missing everywhere.
    let Some((target_uid, target_gid)) = target.ids else {
        eprintln!(
            "sudo: {} has no uid in the account database; refusing to run a command as an account that cannot be named",
            quoteaf_os(&opts.target_user)
        );
        return 1;
    };
    authlib::identity::become_user(&mut cmd, target_uid, target_gid);

    match cmd.status() {
        Ok(status) => status.code().unwrap_or(1),
        Err(e) => {
            eprintln!("sudo: unable to execute {}: {e}", quoteaf_os(&program));
            1
        }
    }
}

/// The program `sudo` runs and its arguments -- which is also what sudoers is
/// asked about, as upstream's `parse_args` arranges it.
///
/// Without `-s` or `-i` that is the command line as given. With either it is
/// the shell: alone, `sudo -s` runs it; with a command, it runs as
/// `SHELL -c CMD`, every byte of the words that is not a letter, a digit, `_`,
/// `-` or `$` escaped with a backslash, so the shell runs the words as typed
/// and splits nothing of its own. Until 2026-10-01 the first WORD was
/// authorised and the unescaped line handed to `sh -c`, so a caller allowed
/// `ls` could run `sudo -s ls '&& id'`.
///
/// `None` when there is nothing to run.
fn invocation(opts: &SudoOpts, target_shell: &str) -> Option<(OsString, Vec<OsString>)> {
    if opts.shell || opts.login_shell {
        let shell = OsString::from(target_shell);
        if opts.command.is_empty() {
            return Some((shell, Vec::new()));
        }
        return Some((
            shell,
            vec![OsString::from("-c"), shell_escaped_command(&opts.command)],
        ));
    }
    let (program, args) = opts.command.split_first()?;
    Some((program.clone(), args.to_vec()))
}

/// The words of a command, joined by single spaces, each byte that is not an
/// ASCII letter or digit, `_`, `-` or `$` preceded by a backslash: what
/// upstream's `parse_args` hands the shell for `sudo -s CMD`.
fn shell_escaped_command(words: &[OsString]) -> OsString {
    let mut out = Vec::new();
    for (i, word) in words.iter().enumerate() {
        if i > 0 {
            out.push(b' ');
        }
        for &b in os_bytes(word).iter() {
            if !(b.is_ascii_alphanumeric() || b == b'_' || b == b'-' || b == b'$') {
                out.push(b'\\');
            }
            out.push(b);
        }
    }
    os_from_bytes(&out)
}

/// Load and parse the sudoers file.
fn load_sudoers() -> Result<SudoersConfig, SudoError> {
    let content = fs::read_to_string(SUDOERS_PATH)
        .map_err(|e| SudoError::InvalidConfig(format!("cannot read {SUDOERS_PATH}: {e}")))?;
    parse_sudoers(&content)
}

/// Main entry point for the `sudoedit` personality.
fn run_sudoedit(files: &[OsString]) -> i32 {
    if files.is_empty() {
        eprintln!("sudoedit: no files specified");
        eprintln!("usage: sudoedit file [file ...]");
        return 1;
    }

    let username = require_username();
    let hostname = require_hostname();
    let user_groups = get_user_groups(&username);

    // Load sudoers to check authorization.
    let config = match load_sudoers() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("sudoedit: {e}");
            return 1;
        }
    };

    // Check authorization: the pseudo-command `sudoedit` with the files as
    // its arguments, as upstream asks it. Until 2026-10-01 a refusal was
    // followed by asking whether the user might RUN each file -- so a rule
    // letting a user run `/etc/motd` let them edit it, while a real
    // `sudoedit /etc/motd` rule counted only if it allowed every file.
    let files_args = user_args(files);
    let request = Request {
        command: b"sudoedit",
        args: files_args.as_deref(),
    };
    if check_authorization(
        &config,
        &username,
        &hostname,
        "root",
        "",
        &request,
        &user_groups,
    )
    .is_none()
    {
        let mut named = OsString::new();
        for (i, file) in files.iter().enumerate() {
            if i > 0 {
                named.push(" ");
            }
            named.push(file);
        }
        eprintln!(
            "sudoedit: {username} is not allowed to edit {} on {hostname}",
            quoteaf_os(&named)
        );
        return 1;
    }

    // The editor runs as the caller, and the copies are theirs: their uid and
    // gid are needed before anything is created.
    let (Some(uid), Some(gid)) = (
        authlib::identity::caller_uid(),
        authlib::identity::caller_gid(),
    ) else {
        eprintln!("sudoedit: unable to determine the invoking user's uid and gid");
        return 1;
    };
    let exit_code = edit_files(files, Caller { uid, gid });

    // Assembled by pushing rather than by `join`, for the same reason as in
    // `run_sudo`: `[OsString]` has no `join`, and joining the lossy forms would
    // record in the audit log a set of filenames other than the ones edited.
    let logged = {
        let mut joined = OsString::from("sudoedit");
        for file in files {
            joined.push(" ");
            joined.push(file);
        }
        joined
    };
    log_command(
        &username,
        &current_tty(),
        &current_pwd(),
        "root",
        &logged,
        if exit_code == 0 { "SUCCESS" } else { "FAILURE" },
    );

    exit_code
}

/// The user who typed `sudoedit`: who the editor runs as and who owns the
/// copies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
// Its fields are read only by the unix half.
#[cfg_attr(not(unix), allow(dead_code))]
struct Caller {
    uid: u32,
    gid: u32,
}

#[cfg(unix)]
/// One file being edited: the original, the copy the editor gets, and the
/// copy's size and time once it was made -- which is how an edit that changed
/// nothing is told from one that did.
struct EditFile {
    original: PathBuf,
    temp: PathBuf,
    before: (u64, Option<SystemTime>),
}

/// `open(2)`'s `O_NOFOLLOW`: refuse a symlink in the last component. Linux's
/// value, which SlateOS's C library shares (a test holds the two together).
#[cfg(unix)]
const O_NOFOLLOW: i32 = 0o400_000;
/// `open(2)`'s `O_NONBLOCK`, so opening a FIFO in a file's place cannot hang.
#[cfg(unix)]
const O_NONBLOCK: i32 = 0o4000;
/// `ELOOP`: what `O_NOFOLLOW` answers for a symlink.
#[cfg(unix)]
const ELOOP: i32 = 40;

/// The directories upstream looks in for one the caller can write, in order.
#[cfg(unix)]
const EDIT_TMPDIRS: [&str; 3] = ["/var/tmp", "/usr/tmp", "/tmp"];

/// Edit `files` for `caller`: upstream's `sudo_edit`, with the editor run as
/// the caller on copies the caller owns.
///
/// # Why each step is there
///
/// An editor run with sudo's privilege is a root shell one `:!sh` away, which
/// is the whole reason sudoedit exists -- and until 2026-10-01 this ran the
/// editor with exactly that privilege, on copies at a predictable
/// `/tmp/sudoedit-<pid>-<name>` that it created by following whatever symlink
/// was waiting there. Now, as upstream does it:
///
/// 1. each original is opened without following a symlink, and refused if
///    any directory on its path is a symlink or writable by the caller, who
///    could otherwise swap the file between the copy and the copy-back; it
///    must be a regular file, and one that does not exist is edited from
///    empty;
/// 2. each copy is created exclusively (`O_EXCL`, so nothing already there is
///    followed or reused) under an unguessable name in the first of
///    `/var/tmp`, `/usr/tmp`, `/tmp` the caller can write, mode 0600, and
///    handed to the caller;
/// 3. one editor runs on all the copies, as the caller;
/// 4. each copy is reopened without following a symlink and must still be a
///    regular file, mode 0600, owned by the caller -- else its original is
///    left alone; a copy whose size and time did not move is reported
///    unchanged; the rest are written over their originals, and a copy that
///    cannot be written back is kept and named.
///
/// The exit status is the editor's, or 1 when a copy could not be written
/// back or nothing could be prepared, as upstream's is.
#[cfg(unix)]
fn edit_files(files: &[OsString], caller: Caller) -> i32 {
    let Some(tmpdir) = edit_tmpdir(caller) else {
        eprintln!("sudoedit: no writable temporary directory found");
        return 1;
    };
    let mut edits = Vec::new();
    for file in files {
        match prepare_edit(Path::new(file), &tmpdir, caller) {
            Ok(edit) => edits.push(edit),
            Err(message) => eprintln!("sudoedit: {message}"),
        }
    }
    if edits.is_empty() {
        return 1;
    }

    let words = editor_words(&editor_command());
    let Some((program, editor_args)) = words.split_first() else {
        return 1;
    };
    let mut cmd = process::Command::new(program);
    cmd.args(editor_args);
    cmd.args(edits.iter().map(|edit| edit.temp.as_os_str()));
    authlib::identity::become_user(&mut cmd, caller.uid, caller.gid);
    let started = SystemTime::now();
    let status = cmd.status();
    let finished = SystemTime::now();
    let mut ret = match status {
        Ok(status) => status.code().unwrap_or(1),
        Err(e) => {
            eprintln!(
                "sudoedit: unable to run {}: {}",
                quoteaf_os(program),
                errmsg::strerror(&e)
            );
            1
        }
    };

    // Copied back whatever the editor's status, as upstream does: an editor
    // that exits non-zero after a save has still saved. Time that did not move
    // means nobody was in the editor, so an unchanged size and time cannot be
    // told from an edit -- and is then copied back, as it is upstream.
    let spent = finished != started;
    for edit in &edits {
        if let Err(message) = copy_back(edit, caller, spent) {
            eprintln!("sudoedit: {message}");
            ret = 1;
        }
    }
    ret
}

/// sudoedit needs to change who the editor runs as, which a host build cannot.
#[cfg(not(unix))]
fn edit_files(_files: &[OsString], _caller: Caller) -> i32 {
    eprintln!("sudoedit: this build cannot run an editor as another user");
    1
}

/// The editor setting split into words, as upstream splits it: `EDITOR="vim
/// -n"` runs `vim` with `-n`. An empty setting is the default editor.
// Called only by the unix half; the host build keeps it for its tests.
#[cfg_attr(not(unix), allow(dead_code))]
fn editor_words(editor: &OsStr) -> Vec<OsString> {
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

/// Whether a directory with this mode, owner and group is writable by
/// `caller`, as upstream's `dir_is_writable` judges it: the caller's own
/// directory always is, and otherwise it is when others may write, or the
/// group may and it is the caller's group. The sticky bit does not make
/// `/tmp` safe to edit in -- the caller can still create there.
///
/// Only the caller's primary group is known here: `userdb` keeps
/// supplementary memberships as names with no name-to-gid resolver (see
/// `authlib::identity`), so a directory writable through one of those is
/// missed -- the direction that refuses less, and recorded as such in
/// known-issues.
// Called only by the unix half; the host build keeps it for its tests.
#[cfg_attr(not(unix), allow(dead_code))]
fn dir_writable_by(mode: u32, owner: u32, group: u32, caller: Caller) -> bool {
    owner == caller.uid || mode & 0o002 != 0 || (mode & 0o020 != 0 && group == caller.gid)
}

/// The first of upstream's temporary directories that the caller can write.
#[cfg(unix)]
fn edit_tmpdir(caller: Caller) -> Option<PathBuf> {
    use std::os::unix::fs::MetadataExt as _;
    EDIT_TMPDIRS.iter().map(PathBuf::from).find(|dir| {
        fs::metadata(dir).is_ok_and(|meta| {
            meta.is_dir() && dir_writable_by(meta.mode(), meta.uid(), meta.gid(), caller)
        })
    })
}

/// Every directory from the root (or the current directory) down to the one
/// holding `original` must be a real directory the caller cannot write:
/// upstream's `sudoedit_checkdir`, and its refusal of a symlinked directory.
#[cfg(unix)]
fn check_path_dirs(original: &Path, caller: Caller) -> Result<(), String> {
    use std::path::Component;

    let name = quoteaf_os(original.as_os_str());
    let parent = original.parent().unwrap_or_else(|| Path::new(""));
    let mut dir = if original.is_absolute() {
        PathBuf::from("/")
    } else {
        PathBuf::from(".")
    };
    check_dir(&dir, &name, caller)?;
    for part in parent.components() {
        match part {
            Component::Normal(step) => dir.push(step),
            Component::ParentDir => dir.push(".."),
            Component::RootDir | Component::CurDir | Component::Prefix(_) => continue,
        }
        check_dir(&dir, &name, caller)?;
    }
    Ok(())
}

/// One step of [`check_path_dirs`]: `dir` must be a directory, not a symlink
/// to one, and not writable by `caller`. `name` is the file being edited,
/// which every refusal names, as upstream's do.
#[cfg(unix)]
fn check_dir(dir: &Path, name: &str, caller: Caller) -> Result<(), String> {
    use std::os::unix::fs::MetadataExt as _;
    let meta =
        fs::symlink_metadata(dir).map_err(|e| format!("{name}: {}", errmsg::strerror(&e)))?;
    if meta.file_type().is_symlink() {
        return Err(format!("{name}: editing symbolic links is not permitted"));
    }
    if !meta.is_dir() {
        return Err(format!("{name}: Not a directory"));
    }
    if dir_writable_by(meta.mode(), meta.uid(), meta.gid(), caller) {
        return Err(format!(
            "{name}: editing files in a writable directory is not permitted"
        ));
    }
    Ok(())
}

/// The original, opened for the copy without following a symlink; `None`
/// when it does not exist yet. Refused unless it is a regular file.
#[cfg(unix)]
fn open_original(original: &Path) -> Result<Option<(fs::File, fs::Metadata)>, String> {
    use std::os::unix::fs::OpenOptionsExt as _;
    let name = quoteaf_os(original.as_os_str());
    match fs::OpenOptions::new()
        .read(true)
        .custom_flags(O_NOFOLLOW | O_NONBLOCK)
        .open(original)
    {
        Ok(file) => {
            let meta = file
                .metadata()
                .map_err(|e| format!("{name}: {}", errmsg::strerror(&e)))?;
            if !meta.is_file() {
                return Err(format!("{name}: not a regular file"));
            }
            Ok(Some((file, meta)))
        }
        Err(e) if e.raw_os_error() == Some(ELOOP) => {
            Err(format!("{name}: editing symbolic links is not permitted"))
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{name}: {}", errmsg::strerror(&e))),
    }
}

/// The name upstream's `sudo_edit_mktemp` gives a copy: the original's base
/// name with eight random characters before its last `.` -- `motd.conf` is
/// `motdXXXXXXXX.conf`, `.bashrc` is `XXXXXXXX.bashrc` -- or after a `.` of
/// their own when there is none, `motd.XXXXXXXX`; so an editor that chooses
/// its mode by suffix still sees the right one.
// Called only by the unix half; the host build keeps it for its tests.
#[cfg_attr(not(unix), allow(dead_code))]
fn temp_name(base: &[u8], random: &[u8]) -> Vec<u8> {
    match base.iter().rposition(|&b| b == b'.') {
        Some(dot) => {
            let (stem, suffix) = base.split_at(dot);
            [stem, random, suffix].concat()
        }
        None => [base, b".", random].concat(),
    }
}

#[cfg(unix)]
/// Eight characters nobody can guess, from the hasher seed the standard
/// library takes from the operating system's randomness.
fn random_letters() -> [u8; 8] {
    use std::hash::{BuildHasher as _, Hasher as _};
    const ALPHABET: &[u8; 62] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    hasher.write_u128(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos()),
    );
    let mut bits = hasher.finish();
    let mut out = [0u8; 8];
    for slot in &mut out {
        let index = usize::try_from(bits % 62).unwrap_or(0);
        *slot = ALPHABET.get(index).copied().unwrap_or(b'X');
        bits /= 62;
    }
    out
}

/// Copy `original` into a new temporary file the caller owns: steps 1 and 2.
#[cfg(unix)]
fn prepare_edit(original: &Path, tmpdir: &Path, caller: Caller) -> Result<EditFile, String> {
    check_path_dirs(original, caller)?;
    prepare_edit_unchecked(original, tmpdir, caller)
}

/// [`prepare_edit`] after its directory check: the original opened, and the
/// copy made. Separate so the tests can make a copy in a directory of their
/// own, which the check rightly refuses.
#[cfg(unix)]
fn prepare_edit_unchecked(
    original: &Path,
    tmpdir: &Path,
    caller: Caller,
) -> Result<EditFile, String> {
    use std::os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _};

    let name = quoteaf_os(original.as_os_str());
    let source = open_original(original)?;

    let base = original
        .file_name()
        .map_or_else(|| b"file".to_vec(), |b| os_bytes(b).into_owned());
    let mut created = None;
    for _ in 0..100 {
        let candidate = tmpdir.join(os_from_bytes(&temp_name(&base, &random_letters())));
        match fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(O_NOFOLLOW)
            .open(&candidate)
        {
            Ok(file) => {
                created = Some((candidate, file));
                break;
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(format!("mkstemps: {}", errmsg::strerror(&e))),
        }
    }
    let Some((temp, mut copy)) = created else {
        return Err("mkstemps: File exists".to_string());
    };
    let remove_on_error = |message: String| -> String {
        // Ignored: the copy is already being abandoned, and a failure to
        // remove it must not replace the reason it was.
        let _ = fs::remove_file(&temp);
        message
    };

    // Exactly 0600, whatever the umask took away: the copy-back insists on it.
    copy.set_permissions(fs::Permissions::from_mode(0o600))
        .map_err(|e| remove_on_error(format!("{name}: {}", errmsg::strerror(&e))))?;
    if let Some((mut file, meta)) = source {
        io::copy(&mut file, &mut copy)
            .map_err(|e| remove_on_error(format!("{name}: {}", errmsg::strerror(&e))))?;
        // The original's time, so the copy-back can tell an untouched copy.
        // Ignored on failure, as upstream ignores it: the time only decides
        // whether to say "unchanged".
        if let Ok(time) = meta.modified() {
            let _ = copy.set_modified(time);
        }
    }
    std::os::unix::fs::fchown(&copy, Some(caller.uid), Some(caller.gid))
        .map_err(|e| remove_on_error(format!("{name}: {}", errmsg::strerror(&e))))?;
    let meta = copy
        .metadata()
        .map_err(|e| remove_on_error(format!("{name}: {}", errmsg::strerror(&e))))?;
    Ok(EditFile {
        original: original.to_path_buf(),
        temp,
        before: (meta.len(), meta.modified().ok()),
    })
}

/// Write one copy back over its original: step 4. `Err` is the message, and
/// the copy is kept for the user unless it could not be trusted.
#[cfg(unix)]
fn copy_back(edit: &EditFile, caller: Caller, spent: bool) -> Result<(), String> {
    use std::os::unix::fs::{MetadataExt as _, OpenOptionsExt as _};

    let name = quoteaf_os(edit.original.as_os_str());
    let temp_shown = quoteaf_os(edit.temp.as_os_str());
    let unmodified = format!("{name} left unmodified");

    let opened = fs::OpenOptions::new()
        .read(true)
        .custom_flags(O_NOFOLLOW | O_NONBLOCK)
        .open(&edit.temp);
    let Ok(mut copy) = opened else {
        return Err(unmodified);
    };
    let Ok(meta) = copy.metadata() else {
        return Err(unmodified);
    };
    // Upstream's `sudo_check_temp_file`, word for word in its warnings.
    if !meta.is_file() {
        eprintln!("sudoedit: {temp_shown}: not a regular file");
        return Err(unmodified);
    }
    if meta.mode() & 0o7777 != 0o600 {
        eprintln!(
            "sudoedit: {temp_shown}: bad file mode: 0{:o}",
            meta.mode() & 0o7777
        );
        return Err(unmodified);
    }
    if meta.uid() != caller.uid {
        eprintln!(
            "sudoedit: {temp_shown} is owned by uid {}, should be {}",
            meta.uid(),
            caller.uid
        );
        return Err(unmodified);
    }

    if spent && (meta.len(), meta.modified().ok()) == edit.before {
        // Ignored: an unchanged copy is litter either way.
        let _ = fs::remove_file(&edit.temp);
        eprintln!("sudoedit: {name} unchanged");
        return Ok(());
    }

    let kept = format!("contents of edit session left in {temp_shown}");
    let mut out = match fs::OpenOptions::new()
        .write(true)
        .create(true)
        .mode(0o644)
        .custom_flags(O_NOFOLLOW)
        .open(&edit.original)
    {
        Ok(out) => out,
        Err(e) => {
            eprintln!(
                "sudoedit: unable to write to {name}: {}",
                errmsg::strerror(&e)
            );
            return Err(kept);
        }
    };
    // Over the old contents, then cut at the new length, as upstream's
    // `sudo_copy_file` does.
    let written = io::copy(&mut copy, &mut out).and_then(|_| out.set_len(meta.len()));
    if let Err(e) = written {
        eprintln!(
            "sudoedit: unable to write to {name}: {}",
            errmsg::strerror(&e)
        );
        return Err(kept);
    }
    // Ignored: the edit is home; a copy left behind is only litter.
    let _ = fs::remove_file(&edit.temp);
    Ok(())
}

/// Main entry point for the `visudo` personality.
fn run_visudo(args: &[OsString]) -> i32 {
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
                eprintln!("visudo: cannot read {}: {e}", quoteaf_os(&opts.file));
                return 1;
            }
        };

        let errors = validate_sudoers(&content, opts.strict);
        if errors.is_empty() {
            println!("{}: parsed OK", quoteaf_os(&opts.file));
            return 0;
        }

        for err in &errors {
            eprintln!("visudo: {}: {err}", quoteaf_os(&opts.file));
        }

        let fatal_count = errors.iter().filter(|e| !e.is_warning).count();
        if fatal_count > 0 {
            return 1;
        }
        if opts.strict {
            return 1;
        }
        println!("{}: parsed with warnings", quoteaf_os(&opts.file));
        return 0;
    }

    // Editing mode: upstream's `visudo`, with the file opened and LOCKED
    // first, the copy beside it, and the result installed by rename.
    edit_sudoers(file_path, opts.strict, &editor_words(&editor_command()))
}

/// The mode a sudoers file is installed with: read-only, owner and group.
#[cfg(unix)]
const SUDOERS_MODE: u32 = 0o440;

/// What becomes of an edit when the copy does not parse: upstream's
/// `whatnow`.
// Called only by the unix half; the host build keeps it for its tests.
#[cfg_attr(not(unix), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WhatNow {
    /// `e`: edit the copy again.
    Edit,
    /// `x`, or the end of input: leave the file as it was.
    Exit,
    /// `Q`: install it anyway.
    Quit,
}

/// One answer to "What now?" -- `None` for anything else, which upstream
/// answers with the list of options and asks again.
// Called only by the unix half; the host build keeps it for its tests.
#[cfg_attr(not(unix), allow(dead_code))]
fn what_now(answer: &str) -> Option<WhatNow> {
    match answer.trim() {
        "e" => Some(WhatNow::Edit),
        "x" => Some(WhatNow::Exit),
        "Q" => Some(WhatNow::Quit),
        _ => None,
    }
}

/// Ask "What now?" until there is an answer. The end of input is `x`, as it
/// is upstream: until 2026-10-01 it was "edit again", forever.
#[cfg(unix)]
fn ask_what_now() -> WhatNow {
    loop {
        eprint!("What now? ");
        // Ignored: a prompt that cannot be shown leaves nothing better to do
        // than read the answer anyway.
        let _ = io::stderr().flush();
        let mut answer = String::new();
        match io::stdin().read_line(&mut answer) {
            Ok(0) | Err(_) => return WhatNow::Exit,
            Ok(_) => {}
        }
        if let Some(choice) = what_now(&answer) {
            return choice;
        }
        eprintln!(
            "Options are:\n  (e)dit sudoers file again\n  e(x)it without saving changes to sudoers file\n  (Q)uit and save changes to sudoers file (DANGER!)\n"
        );
    }
}

/// The name of the copy `visudo` edits: upstream's `<file>.tmp`, beside the
/// file, so the install is a rename within one directory -- atomic, and never
/// a moment with half a sudoers file -- and in a directory only root writes,
/// unlike `/tmp`, where the copy used to be made at a name anyone could
/// predict and plant a symlink on.
// Called only by the unix half; the host build keeps it for its tests.
#[cfg_attr(not(unix), allow(dead_code))]
fn sudoers_temp_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(".tmp");
    PathBuf::from(name)
}

/// The contents as the copy should start: the file's own, with a final
/// newline if it lacked one, as upstream adds it.
// Called only by the unix half; the host build keeps it for its tests.
#[cfg_attr(not(unix), allow(dead_code))]
fn sudoers_starting_text(original: &[u8]) -> Vec<u8> {
    let mut text = original.to_vec();
    if text.last().is_some_and(|&b| b != b'\n') {
        text.push(b'\n');
    }
    text
}

/// Edit the sudoers file at `path`: upstream's `visudo` editing loop.
///
/// Until 2026-10-01 the lock was a `.lck` file that was checked for and then
/// written, so two `visudo`s could both take it, and one more than five
/// minutes old was "stale" and taken while its owner was still editing; the
/// copy was made at `/tmp/visudo-<pid>`, a name anyone could predict and point
/// at another file with a symlink; and the result was written over the file
/// in place, with no mode or owner set. Now:
///
/// 1. the file itself is opened (created 0440 when absent) and locked with
///    `flock`, held until `visudo` is done; a held lock is "busy, try again
///    later";
/// 2. the copy is `<file>.tmp`, opened without following a symlink, holding
///    the file's text with a final newline and the file's time;
/// 3. the editor runs on it (`EDITOR -- file.tmp`); a copy left empty where the
///    file was not is refused; one whose size and time did not move, with time
///    spent in the editor, is "unchanged" and nothing is installed;
/// 4. a copy that does not parse is reported and the choice offered --
///    (e)dit again, e(x)it, or (Q)uit and save;
/// 5. a copy that does is given to root:root, mode 0440, and renamed over the
///    file.
#[cfg(unix)]
fn edit_sudoers(path: &Path, strict: bool, editor: &[OsString]) -> i32 {
    use std::os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _};

    let shown = quoteaf_os(path.as_os_str());
    let mut sudoers = match fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .mode(SUDOERS_MODE)
        .open(path)
    {
        Ok(file) => file,
        Err(e) => {
            eprintln!("visudo: {shown}: {}", errmsg::strerror(&e));
            return 1;
        }
    };
    match sudoers.try_lock() {
        Ok(()) => {}
        Err(fs::TryLockError::WouldBlock) => {
            eprintln!("visudo: {shown} busy, try again later");
            return 1;
        }
        Err(fs::TryLockError::Error(e)) => {
            eprintln!("visudo: unable to lock {shown}: {}", errmsg::strerror(&e));
            eprint!("Edit anyway? [y/N]");
            // Ignored, as the prompt above it: the answer is read either way.
            let _ = io::stderr().flush();
            let mut answer = String::new();
            let yes = io::stdin().read_line(&mut answer).is_ok()
                && answer.trim_start().starts_with(['y', 'Y']);
            if !yes {
                return 1;
            }
        }
    }

    let mut original = Vec::new();
    let original_meta = match sudoers
        .read_to_end(&mut original)
        .and_then(|_| sudoers.metadata())
    {
        Ok(meta) => meta,
        Err(e) => {
            eprintln!("visudo: {shown}: {}", errmsg::strerror(&e));
            return 1;
        }
    };

    let temp = sudoers_temp_path(path);
    let temp_shown = quoteaf_os(temp.as_os_str());
    let discard = |code: i32| -> i32 {
        // Ignored: the copy is being abandoned, and the reason has been said.
        let _ = fs::remove_file(&temp);
        code
    };
    {
        let made = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o700)
            .custom_flags(O_NOFOLLOW)
            .open(&temp)
            .and_then(|mut copy| {
                copy.write_all(&sudoers_starting_text(&original))?;
                // The file's time, so an untouched copy can be told; ignored on
                // failure, as upstream ignores it.
                if let Ok(time) = original_meta.modified() {
                    let _ = copy.set_modified(time);
                }
                Ok(())
            });
        if let Err(e) = made {
            eprintln!("visudo: {temp_shown}: {}", errmsg::strerror(&e));
            return discard(1);
        }
    }

    let Some((editor, editor_args)) = editor.split_first() else {
        return discard(1);
    };
    loop {
        let started = SystemTime::now();
        let ran = process::Command::new(editor)
            .args(editor_args)
            .arg("--")
            .arg(&temp)
            .status();
        let finished = SystemTime::now();
        // vi's exit status counts its errors (XPG4), so only a failure to run
        // the editor at all stops here -- as upstream.
        if ran.is_err() {
            eprintln!(
                "visudo: editor ({}) failed, {shown} unchanged",
                quoteaf_os(editor)
            );
            return discard(1);
        }
        let edited = match fs::metadata(&temp) {
            Ok(meta) => meta,
            Err(_) => {
                eprintln!(
                    "visudo: unable to stat temporary file ({temp_shown}), {shown} unchanged"
                );
                return discard(1);
            }
        };
        if edited.len() == 0 && !original.is_empty() {
            eprintln!("visudo: zero length temporary file ({temp_shown}), {shown} unchanged");
            return discard(1);
        }
        let untouched = edited.len() == original_meta.len()
            && edited.modified().ok() == original_meta.modified().ok()
            && finished != started;
        if untouched {
            eprintln!("visudo: {temp_shown} unchanged");
            return discard(0);
        }

        let text = match fs::read_to_string(&temp) {
            Ok(text) => text,
            Err(e) => {
                eprintln!("visudo: {temp_shown}: {}", errmsg::strerror(&e));
                return discard(1);
            }
        };
        let errors = validate_sudoers(&text, strict);
        let fatal: Vec<&SyntaxError> = errors.iter().filter(|e| !e.is_warning).collect();
        if !fatal.is_empty() {
            for err in &fatal {
                eprintln!("visudo: {temp_shown}: {err}");
            }
            match ask_what_now() {
                WhatNow::Edit => continue,
                WhatNow::Exit => return discard(0),
                WhatNow::Quit => {}
            }
        }

        // Install: root's, 0440, renamed over the file.
        // The order matters: owner and mode are set on the copy BEFORE the
        // rename, so the file is never readable under the wrong ones.
        if let Err(e) = std::os::unix::fs::chown(&temp, Some(0), Some(0)) {
            eprintln!(
                "visudo: unable to set (uid, gid) of {temp_shown} to (0, 0): {}",
                errmsg::strerror(&e)
            );
        }
        if let Err(e) = fs::set_permissions(&temp, fs::Permissions::from_mode(SUDOERS_MODE)) {
            eprintln!(
                "visudo: unable to change mode of {temp_shown} to 0{SUDOERS_MODE:o}: {}",
                errmsg::strerror(&e)
            );
        }
        if let Err(e) = fs::rename(&temp, path) {
            eprintln!(
                "visudo: error renaming {temp_shown}, {shown} unchanged: {}",
                errmsg::strerror(&e)
            );
            return discard(1);
        }
        return 0;
    }
}

/// `visudo` installs root's file and must be able to change owners; a host
/// build cannot.
#[cfg(not(unix))]
fn edit_sudoers(_path: &Path, _strict: bool, _editor: &[OsString]) -> i32 {
    eprintln!("visudo: this build cannot install a sudoers file");
    1
}

/// Main entry point for the `sudoreplay` personality.
fn run_sudoreplay(args: &[OsString]) -> i32 {
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
            println!(
                "No recorded sessions found in {}",
                quoteaf_os(&opts.directory)
            );
            return 0;
        }

        println!(
            "{:<12} {:<12} {:<12} {:<20} COMMAND",
            "SESSION", "USER", "RUNAS", "DATE"
        );
        println!("{}", "-".repeat(76));

        for session in &sessions {
            let date = format_timestamp(session.timestamp);
            // `escape_os`, not the raw name: this is a fixed-width table, and
            // a session directory named with a newline or a tab would otherwise
            // rewrite the rows below it. A name that is already plain text
            // comes through `escape_os` unchanged, so the ordinary listing is
            // exactly as it was.
            println!(
                "{:<12} {:<12} {:<12} {:<20} {}",
                escape_os(&session.id),
                session.user,
                session.target_user,
                date,
                session.command
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
    // `args_os`, not `args`. `env::args()`'s iterator is documented to panic on
    // an argument that is not valid Unicode and its body is a literal `unwrap`,
    // so `sudo /usr/local/bin/tool\xff` did not deny the command, did not log
    // it, and did not reach a single line this crate wrote -- it aborted with a
    // Rust panic before `main`'s first statement. On this OS a path may hold
    // every byte but `/` and NUL (`design.txt`), so that argument is legal and
    // the panic was reachable by an ordinary filename. See
    // `known-issues.md` -> `B-COREUTILS-PANIC-ON-A-NON-UTF-8-ARGUMENT` and the
    // ratchet in `scripts/argv-utf8.py`.
    let args: Vec<OsString> = env::args_os().collect();

    // `detect_personality` takes the basename and strips `.exe` itself, so the
    // hand-rolled copy of that logic which used to stand here was a second
    // implementation of one rule -- the rule that decides whether this process
    // behaves as `sudo` or as `visudo`. Pass argv[0] through whole, and take
    // the remainder from the same split, so the two cannot disagree about
    // where the arguments begin.
    let (argv0, rest) = args
        .split_first()
        .map_or((OsStr::new("sudo"), [].as_slice()), |(argv0, rest)| {
            (argv0.as_os_str(), rest)
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
// Panicking on bad data is what a test is *for*, which is why CLAUDE.md allows
// these here and nowhere else. Without the attribute this module alone emits a
// dozen warnings, and a clippy run with a standing dozen warnings is one whose
// thirteenth — a real one, in production code — goes unread.
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;
    use scratchdir::ScratchDir;

    /// The secure path the command-spec tests resolve against. Absolute and
    /// fabricated: nothing here exists on the build host, so a test that
    /// expects a bare spec to RESOLVE must plant a real file and say so.
    const SECURE: [&str; 2] = ["/usr/bin", "/bin"];

    // -- The credential cache's clock --
    //
    // `check_timestamp` decides whether to skip the password prompt. Its own
    // doc comment reasoned carefully about every failure arm and was checked
    // by a 2026-09-10 sweep, which recorded that it "is correct as written"
    // and need not be re-derived. Both defects below are in arms that sweep
    // was not looking at, and in neither does anything fail.

    #[test]
    fn a_fresh_timestamp_is_fresh_and_a_stale_one_is_not() {
        assert!(
            timestamp_is_fresh("1000\n", 1010, 300),
            "10s old, 300s timeout"
        );
        assert!(!timestamp_is_fresh("1000\n", 2000, 300), "1000s old");
    }

    #[test]
    fn the_boundary_is_strictly_less_than_the_timeout() {
        // Exactly `timeout` seconds old has expired; one second less has not.
        assert!(!timestamp_is_fresh("1000\n", 1300, 300));
        assert!(timestamp_is_fresh("1000\n", 1299, 300));
    }

    #[test]
    fn a_timestamp_in_the_future_is_not_a_fresh_authentication() {
        // `now.saturating_sub(ts)` answered 0 and `0 < timeout` is true, so
        // the prompt was skipped until the clock caught up. A backwards clock
        // step -- an NTP correction, an RTC read at boot, a restored VM
        // snapshot -- produces exactly this, and nothing fails while it does.
        assert!(!timestamp_is_fresh("9999\n", 1000, 300));
        assert!(
            !timestamp_is_fresh("1001\n", 1000, 300),
            "even by one second"
        );
    }

    #[test]
    fn never_expires_still_does_not_trust_a_future_timestamp() {
        // `timestamp_timeout=-1` says how long an authentication lasts. It is
        // not a licence to believe a file whose contents cannot be true.
        assert!(
            timestamp_is_fresh("1000\n", 5000, u64::MAX),
            "past, never expires"
        );
        assert!(
            !timestamp_is_fresh("9999\n", 1000, u64::MAX),
            "future, refused"
        );
    }

    #[test]
    fn an_unparsable_or_empty_timestamp_demands_the_password() {
        assert!(!timestamp_is_fresh("", 1000, 300));
        assert!(!timestamp_is_fresh("not a number\n", 1000, 300));
        assert!(
            !timestamp_is_fresh("-1\n", 1000, 300),
            "negative is not a u64"
        );
    }

    #[test]
    fn invalidation_survives_a_never_expires_timeout() {
        // `invalidate_timestamp` writes "0" to drop a cached credential --
        // `sudo -k`. Under a finite timeout that is 1970 and long expired, so
        // it worked. Under `timestamp_timeout=-1` NOTHING expires, so the
        // sentinel came back FRESH and `sudo -k` was a no-op for exactly the
        // configuration where the credential otherwise lasts all session.
        //
        // Found by writing the test above and then reading what it asserted:
        // its first draft encoded the broken answer as the expected one, which
        // is how a defect becomes a fixture.
        assert!(!timestamp_is_fresh("0\n", 1_000_000, 300));
        assert!(!timestamp_is_fresh("0\n", 1_000_000, u64::MAX));
    }

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
        authlib::Authenticator::with_stores(missing)
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
        locked
            .find_mut("alice")
            .expect("alice exists")
            .set_locked(true);
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

    /// A clock that does not move, for the test below.
    ///
    /// It has to be a `fn()` because that is what `with_clock` takes, and it
    /// can be a *constant* one — unlike authlib's own rate-limit tests, which
    /// need a settable static because they check that a delay expires. This
    /// test never advances time, so there is no cell for a concurrently
    /// running test to write and nothing to serialise.
    fn frozen_now() -> u64 {
        1_000_000
    }

    /// The point of the whole exercise: a delay earned at another prompt is
    /// honoured here. `Authenticator::with_faillock` is what `login`, `su` and
    /// the greeter share on a real system; two authenticators pointed at one
    /// file stand in for two programs.
    #[test]
    fn sudo_honours_a_delay_earned_at_another_prompt() {
        // A directory unique per process *and* per thread, removed when `dir`
        // drops — including on an unwind out of a failed assertion below, which
        // is the case a cleanup line in the test's tail can never cover.
        let dir = scratchdir::ScratchDir::new("sudo_faillock_share");
        let faillock = dir.path("faillock");
        let missing = std::path::Path::new("/nonexistent/sudo-tests");

        // Both halves read one frozen clock. Against the real one this test
        // failed about one run in three, and correctly so: the tally is stamped
        // in whole seconds, and the loop below stops at the *first* delay it
        // earns, which is `1 << 0` — one second. What survives to the assertion
        // is therefore not a second but whatever is left of the current one, so
        // a ~0.1s test starting near a second boundary watches the delay expire
        // before it can check for it. The production code was right; the
        // question was time-dependent. Pinning the clock removes the dependency
        // without weakening what is proved — that two authenticators over one
        // faillock file share a tally. Reported by lane C in
        // `requests/c-b-sudo-faillock-sharing-test-races-the-wall-clock.md`.

        // `login` (or `su`, or the greeter) burns through the allowance.
        {
            let mut elsewhere = authlib::Authenticator::with_stores(missing)
                .with_faillock(&faillock)
                .with_clock(frozen_now);
            while elsewhere.rate_limited("alice").is_none() {
                elsewhere.note_failure("alice");
            }
        }

        // `sudo` starts fresh, reads the same file, and refuses.
        let db = auth_fixture();
        let mut auth = authlib::Authenticator::with_stores(missing)
            .with_faillock(&faillock)
            .with_clock(frozen_now);
        // Naming the number, not just `is_some()`: the loop above stops at the
        // *first* delay earned, which is `1 << 0`, so with the clock pinned the
        // whole of it survives and the answer is determinate. `is_some()` would
        // pass just as quietly on a delay mis-computed as sixty — the
        // quiet-pass direction lane C flagged alongside the flake.
        assert_eq!(
            auth.rate_limited("alice"),
            Some(1),
            "a delay earned at another prompt must be honoured here, in full"
        );
        assert!(
            authenticate_against(&mut auth, &db, "alice", "hunter2").is_err(),
            "the correct password must not skip a wait earned elsewhere"
        );
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
            &Request::bare(b"/usr/bin/ls"),
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
            &Request::bare(b"/usr/bin/ls"),
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
            &Request::bare(b"/usr/bin/apt"),
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
            &Request::bare(b"/usr/bin/rm"),
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
            &Request::bare(b"/usr/bin/ls"),
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
            &Request::bare(b"/usr/bin/ls"),
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
            &Request::bare(b"/usr/bin/ls"),
            &["alice".to_string()],
        );
        assert!(result.is_some());
    }

    /// `$TTY` must not name the terminal the audit record blames.
    #[test]
    fn the_environment_cannot_name_the_terminal_in_the_log() {
        // SAFETY: single-threaded test and the variable is removed again
        // below; `set_var` is unsafe in edition 2024 only because a concurrent
        // reader would be UB.
        unsafe {
            std::env::set_var("TTY", "attacker-chosen");
        }
        let answer = current_tty();
        unsafe {
            std::env::remove_var("TTY");
        }
        assert_ne!(answer, OsString::from("attacker-chosen"));
    }

    /// `$USER` must not reach the name sudo authorises, authenticates and
    /// caches credentials under.
    ///
    /// The cache is what makes this escalation rather than mis-attribution:
    /// `timestamp_path` is `TIMESTAMP_DIR/<username>` and `check_timestamp`
    /// skips the password prompt when that file is fresh, so a forged name
    /// borrows whoever last ran sudo.
    #[test]
    fn the_environment_cannot_choose_who_you_are() {
        // SAFETY: single-threaded test; both variables are removed again
        // below. `set_var` is unsafe in edition 2024 because a concurrent
        // reader would be UB, and there is no other thread here.
        unsafe {
            std::env::set_var("USER", "attacker-chosen");
            std::env::set_var("LOGNAME", "attacker-chosen-too");
        }
        let answer = current_username();
        unsafe {
            std::env::remove_var("USER");
            std::env::remove_var("LOGNAME");
        }
        assert_ne!(answer.as_deref(), Some("attacker-chosen"));
        assert_ne!(answer.as_deref(), Some("attacker-chosen-too"));
    }

    /// The credential cache path must not be reachable from a name the caller
    /// supplied -- and must not escape its directory even if one ever were.
    #[test]
    fn a_timestamp_path_stays_under_its_directory() {
        let ours = timestamp_path("alice");
        assert!(ours.starts_with(TIMESTAMP_DIR), "{ours:?}");
        assert!(ours.ends_with("alice"), "{ours:?}");
        // Different users do not share a cache; sharing one is the same bug
        // as letting the name be chosen.
        assert_ne!(timestamp_path("alice"), timestamp_path("bob"));
    }

    /// `$HOSTNAME` must not reach the hostname the sudoers rules are matched
    /// against. It used to: the lookup fell through `/etc/hostname` to the
    /// environment, so `HOSTNAME=web01 sudo ...` selected `web01`'s rules.
    #[test]
    fn the_environment_cannot_choose_which_rules_apply() {
        // SAFETY: single-threaded test, and the variable is removed again
        // below. `set_var` is unsafe in edition 2024 because another thread
        // reading the environment concurrently is UB; there is no other
        // thread here.
        unsafe {
            std::env::set_var("HOSTNAME", "attacker-chosen");
        }
        let answer = current_hostname();
        unsafe {
            std::env::remove_var("HOSTNAME");
        }
        // On a host with no /proc/sys/kernel/hostname this is None, and on one
        // with it the real name. Either way it is never what the caller put in
        // the environment.
        assert_ne!(answer.as_deref(), Some("attacker-chosen"));
    }

    /// An empty hostname file is "I do not know", not the empty host.
    ///
    /// `host_matches` compares with `==`, so an empty name would match a
    /// sudoers spec written as `""` and, more to the point, would silently not
    /// match anything real -- a denial whose cause is invisible.
    #[test]
    fn an_empty_name_is_not_a_hostname() {
        // The parse half of the lookup, exercised directly: this is what the
        // function does with the file's contents once read.
        for raw in [
            "", "   ", "
", "	
 ",
        ] {
            assert!(raw.trim().is_empty(), "fixture {raw:?} must be blank");
        }
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
            &Request::bare(b"/usr/bin/ls"),
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
            &Request::bare(b"/usr/bin/ls"),
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
            &Request::bare(b"/usr/bin/ls"),
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
            &Request::bare(b"/usr/bin/ls"),
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
            &Request::bare(b"/sbin/ifconfig"),
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
            &Request::bare(b"/usr/bin/apt"),
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
            &Request::bare(b"/usr/bin/ls"),
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
            &Request::bare(b"/usr/bin/anything"),
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
            &Request::bare(b"/usr/sbin/something"),
            &["alice".to_string()],
        );
        assert!(result.is_none());
    }

    // -- Command matching tests --

    #[test]
    fn command_match_exact() {
        assert!(command_matches(
            "/usr/bin/ls",
            "",
            &Request::bare(b"/usr/bin/ls"),
            &SECURE
        ));
    }

    #[test]
    fn command_match_all() {
        assert!(command_matches(
            "ALL",
            "",
            &Request::bare(b"/any/command"),
            &SECURE
        ));
    }

    #[test]
    fn command_path_match_wildcard() {
        assert!(command_path_matches("/usr/bin/*", b"/usr/bin/ls", &SECURE));
        assert!(command_path_matches("/usr/bin/*", b"/usr/bin/cat", &SECURE));
    }

    #[test]
    fn command_path_no_match_wildcard() {
        assert!(!command_path_matches(
            "/usr/bin/*",
            b"/usr/sbin/ls",
            &SECURE
        ));
    }

    #[test]
    fn command_path_basename_match() {
        // An unqualified spec is RESOLVED now, so this needs a real file --
        // and a fabricated `/usr/bin` would make every assertion below pass
        // for the wrong reason.
        let dir = ScratchDir::new("sudo_cmdspec");
        let path = dir.dir().to_string_lossy().to_string();
        let real = format!("{path}/ls");
        fs::write(&real, b"x").expect("write fixture");
        let dirs = [path.as_str()];

        assert!(command_path_matches("ls", real.as_bytes(), &dirs));
        // And the caller may type the bare name: both sides resolve.
        assert!(command_path_matches("ls", b"ls", &dirs));
    }

    /// The escalation this replaced, kept as the thing that must stay false.
    ///
    /// `command_path_matches` compared the last path component alone and
    /// `actual_cmd` is the caller's argv verbatim, so `alice ALL = ls`
    /// authorised `sudo /tmp/evil/ls` -- a binary the caller put there, run as
    /// root. Asserted true here on 2026-09-12 before the fix.
    #[test]
    fn an_unqualified_spec_does_not_authorise_a_path_the_caller_chose() {
        let dir = ScratchDir::new("sudo_cmdspec");
        let path = dir.dir().to_string_lossy().to_string();
        fs::write(format!("{path}/ls"), b"x").expect("write fixture");
        let dirs = [path.as_str()];

        assert!(!command_path_matches("ls", b"/tmp/evil/ls", &dirs));
        assert!(!command_path_matches("ls", b"./ls", &dirs));
        assert!(!command_path_matches("ls", b"/home/alice/ls", &dirs));
    }

    #[test]
    fn an_unqualified_spec_naming_nothing_on_the_secure_path_matches_nothing() {
        // The safe direction: a rule that cannot be resolved authorises
        // nothing rather than authorising by name.
        let dir = ScratchDir::new("sudo_cmdspec");
        let path = dir.dir().to_string_lossy().to_string();
        let dirs = [path.as_str()];
        assert!(!command_path_matches(
            "nosuchtool",
            b"/usr/bin/nosuchtool",
            &dirs
        ));
    }

    #[test]
    fn secure_path_defaults_and_is_overridable() {
        // `secure_path` was parsed and stored and never read until
        // 2026-09-12. This is its first consumer.
        let config = parse_sudoers("").expect("empty sudoers parses");
        assert!(secure_path_of(&config).contains("/usr/bin"));
        let config = parse_sudoers(
            "Defaults secure_path = /opt/bin
",
        )
        .expect("parses");
        assert_eq!(secure_path_of(&config), "/opt/bin");
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

    /// A negation written after a broader entry is the whole point of the
    /// idiom, and was unreachable: every list matcher walked forward and
    /// returned at the first match, so `ALL` answered before `!secret` was
    /// looked at. Asserted TRUE here on 2026-09-12 to prove it, before the
    /// fix; it is the same three lines inverted.
    #[test]
    fn a_negated_host_after_all_is_honoured() {
        let aliases = HashMap::new();
        let specs = ["ALL".to_string(), "!secret".to_string()];
        assert!(!host_matches(&specs, "secret", &aliases));
        // ...and the rule still applies everywhere else.
        assert!(host_matches(&specs, "web1", &aliases));
    }

    #[test]
    fn a_negated_user_after_all_is_honoured() {
        let aliases = HashMap::new();
        let specs = ["ALL".to_string(), "!mallory".to_string()];
        assert!(!user_matches(&specs, "mallory", &[], &aliases));
        assert!(user_matches(&specs, "alice", &[], &aliases));
    }

    #[test]
    fn a_negated_runas_target_is_honoured() {
        // `runas_matches` used `.any()` and read `!` as part of a name, so
        // `(ALL, !root)` permitted running things AS root -- the one target it
        // was written to forbid.
        let aliases = HashMap::new();
        let runas = RunasSpec {
            users: vec!["ALL".to_string(), "!root".to_string()],
            groups: Vec::new(),
        };
        assert!(!runas_matches(&runas, "root", "", &aliases));
        assert!(runas_matches(&runas, "backup", "", &aliases));
    }

    #[test]
    fn order_decides_and_the_last_word_wins() {
        // The rule this implements. Reversing the list reverses the answer,
        // which is what "last match wins" means and what first-match-wins
        // could not express.
        let aliases = HashMap::new();
        let deny_last = ["ALL".to_string(), "!secret".to_string()];
        let allow_last = ["!secret".to_string(), "ALL".to_string()];
        assert!(!host_matches(&deny_last, "secret", &aliases));
        assert!(host_matches(&allow_last, "secret", &aliases));
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
        assert!(
            keep.contains(&"TERM".to_string()),
            "`+=` keeps the built-ins"
        );
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

    /// An argv, in the form the parsers take it.
    ///
    /// The parsers take `[OsString]` because argv *is* bytes on this system
    /// (see [`SudoOpts`]), but a fixture that spells `OsString::from` at every
    /// element is a fixture nobody adds a case to — these used to say
    /// `.to_string()` at every element for exactly that reason, and it would be
    /// the same problem in a new type. `argv(&["-u", "bob", "ls"])` reads as the
    /// command line it stands for.
    fn argv(parts: &[&str]) -> Vec<OsString> {
        parts.iter().map(OsString::from).collect()
    }

    /// The same, for arguments that are *not* valid UTF-8 and so cannot be
    /// written as a `&str` at all — the arguments that used to abort the
    /// process inside `env::args()` before `main` ran a line.
    ///
    /// **Unix only, because off unix it would silently test nothing.**
    /// `quoting::os_from_bytes` is byte-exact under `#[cfg(unix)]`, where an
    /// `OsStr` *is* its bytes; on Windows an `OsString` is UTF-16-shaped and the
    /// same call goes through `String::from_utf8_lossy`, so `b"tool\xff"`
    /// arrives as `tool\u{fffd}` — valid text, and the very substitution these
    /// tests exist to rule out. A byte-exactness assertion built on it would
    /// have compared the host's lossy conversion against itself and passed.
    /// Use [`not_text`] for the cases that only need an argument which is not
    /// valid Unicode; it works on both platforms.
    #[cfg(unix)]
    fn argv_bytes(parts: &[&[u8]]) -> Vec<OsString> {
        parts.iter().map(|b| os_from_bytes(b)).collect()
    }

    /// An `OsString` spelled `prefix`, then one unit that is **not valid
    /// Unicode**, then `suffix`.
    ///
    /// This exists so that the "argv need not be text" cases run on the
    /// development host as well as on the target, which matters more here than
    /// anywhere else: Windows is where this code is actually built and tested,
    /// and a case gated out on Windows is a case that only runs where nobody
    /// looks. Each platform gets the cheapest thing its `OsString` can hold that
    /// `to_str()` refuses:
    ///
    /// - unix: byte `0xff`, which begins no UTF-8 sequence.
    /// - Windows: `U+D800`, an unpaired surrogate. `OsString` there is WTF-8 and
    ///   stores it happily; `to_str()` returns `None`, exactly as for the byte.
    ///
    /// The two are not the same value and no test may assume they are — a test
    /// that needs *specific bytes* wants [`argv_bytes`] and `#[cfg(unix)]`.
    /// What every caller here needs instead is weaker and platform-free: an
    /// argument that is not text, carried through unchanged.
    fn not_text(prefix: &str, suffix: &str) -> OsString {
        #[cfg(unix)]
        let out = {
            let mut v = prefix.as_bytes().to_vec();
            v.push(0xff);
            v.extend_from_slice(suffix.as_bytes());
            os_from_bytes(&v)
        };
        #[cfg(not(unix))]
        let out = {
            use std::os::windows::ffi::OsStringExt;
            let mut w: Vec<u16> = prefix.encode_utf16().collect();
            w.push(0xD800);
            w.extend(suffix.encode_utf16());
            OsString::from_wide(&w)
        };
        // Whatever it is made of, it must be the thing the caller asked for:
        // something no `&str` can hold. A platform whose fixture quietly became
        // valid text would make every test below assert nothing.
        assert!(out.to_str().is_none(), "the fixture must not be valid text");
        out
    }

    #[test]
    fn parse_sudo_args_simple_command() {
        let args = argv(&["ls", "-la"]);
        let opts = parse_sudo_args(&args).unwrap();
        assert_eq!(opts.command, vec!["ls", "-la"]);
        assert_eq!(opts.target_user, "root");
    }

    #[test]
    fn parse_sudo_args_target_user() {
        let args = argv(&["-u", "bob", "ls"]);
        let opts = parse_sudo_args(&args).unwrap();
        assert_eq!(opts.target_user, "bob");
        assert_eq!(opts.command, vec!["ls"]);
    }

    #[test]
    fn parse_sudo_args_target_group() {
        let args = argv(&["-g", "staff", "ls"]);
        let opts = parse_sudo_args(&args).unwrap();
        assert_eq!(opts.target_group, "staff");
    }

    #[test]
    fn parse_sudo_args_login_shell() {
        let args = argv(&["-i"]);
        let opts = parse_sudo_args(&args).unwrap();
        assert!(opts.login_shell);
        assert!(opts.command.is_empty());
    }

    #[test]
    fn parse_sudo_args_shell() {
        let args = argv(&["-s", "ls"]);
        let opts = parse_sudo_args(&args).unwrap();
        assert!(opts.shell);
    }

    #[test]
    fn parse_sudo_args_list() {
        let args = argv(&["-l"]);
        let opts = parse_sudo_args(&args).unwrap();
        assert!(opts.list);
    }

    #[test]
    fn parse_sudo_args_validate() {
        let args = argv(&["-v"]);
        let opts = parse_sudo_args(&args).unwrap();
        assert!(opts.validate);
    }

    #[test]
    fn parse_sudo_args_invalidate() {
        let args = argv(&["-k"]);
        let opts = parse_sudo_args(&args).unwrap();
        assert!(opts.invalidate);
    }

    #[test]
    fn parse_sudo_args_remove_timestamp() {
        let args = argv(&["-K"]);
        let opts = parse_sudo_args(&args).unwrap();
        assert!(opts.remove_timestamp);
    }

    #[test]
    fn parse_sudo_args_non_interactive() {
        let args = argv(&["-n", "ls"]);
        let opts = parse_sudo_args(&args).unwrap();
        assert!(opts.non_interactive);
    }

    #[test]
    fn parse_sudo_args_background() {
        let args = argv(&["-b", "sleep", "60"]);
        let opts = parse_sudo_args(&args).unwrap();
        assert!(opts.background);
    }

    #[test]
    fn parse_sudo_args_edit() {
        let args = argv(&["-e", "/etc/hosts"]);
        let opts = parse_sudo_args(&args).unwrap();
        assert!(opts.edit_mode);
    }

    #[test]
    fn parse_sudo_args_preserve_env() {
        let args = argv(&["-E", "ls"]);
        let opts = parse_sudo_args(&args).unwrap();
        assert!(opts.preserve_env);
    }

    #[test]
    fn parse_sudo_args_custom_prompt() {
        let args = argv(&["-p", "Enter: ", "ls"]);
        let opts = parse_sudo_args(&args).unwrap();
        assert_eq!(opts.prompt, "Enter: ");
    }

    #[test]
    fn parse_sudo_args_double_dash() {
        let args = argv(&["-u", "root", "--", "-k"]);
        let opts = parse_sudo_args(&args).unwrap();
        assert_eq!(opts.command, vec!["-k"]);
        assert!(!opts.invalidate);
    }

    #[test]
    fn parse_sudo_args_combined_flags() {
        let args = argv(&["-inE", "ls"]);
        let opts = parse_sudo_args(&args).unwrap();
        assert!(opts.login_shell);
        assert!(opts.non_interactive);
        assert!(opts.preserve_env);
    }

    #[test]
    fn parse_sudo_args_unknown_flag() {
        let args = argv(&["-Z"]);
        assert!(parse_sudo_args(&args).is_err());
    }

    #[test]
    fn parse_sudo_args_u_missing_value() {
        let args = argv(&["-u"]);
        assert!(parse_sudo_args(&args).is_err());
    }

    #[test]
    fn parse_sudo_args_empty() {
        let args: Vec<OsString> = vec![];
        let opts = parse_sudo_args(&args).unwrap();
        assert!(opts.command.is_empty());
        assert_eq!(opts.target_user, "root");
    }

    // -- argv is bytes --
    //
    // The whole point of the `OsString` conversion. Every case here was, before
    // it, either a process abort inside `env::args()` before `main` ran a line,
    // or a silent substitution of U+FFFD for a byte -- in the one crate whose
    // job is to decide which user runs which program.

    #[test]
    fn a_command_that_is_not_utf8_survives_parsing_unchanged() {
        let wanted = not_text("/usr/local/bin/tool", "");
        let opts = parse_sudo_args(std::slice::from_ref(&wanted)).unwrap();
        assert_eq!(opts.command, vec![wanted]);
    }

    /// The byte-exact form of the case above. Only on unix, because only there
    /// is an `OsStr` its bytes; see [`argv_bytes`]. This is the assertion that
    /// matters on the target, where the command is eventually handed to `exec`
    /// as bytes and one substituted byte is a different program.
    #[cfg(unix)]
    #[test]
    fn a_command_that_is_not_utf8_survives_parsing_byte_for_byte() {
        let args = argv_bytes(&[b"/usr/local/bin/tool\xff"]);
        let opts = parse_sudo_args(&args).unwrap();
        assert_eq!(opts.command.len(), 1);
        assert_eq!(&*os_bytes(&opts.command[0]), b"/usr/local/bin/tool\xff");
    }

    /// Two commands differing only in a unit no `String` can hold must stay two
    /// commands. Had either gone through `to_string_lossy`, both would render
    /// as `tool\u{fffd}` and authorising one would authorise the other.
    ///
    /// Spelled with the *prefix* varying rather than the non-text unit, so that
    /// it says the same thing on both platforms: [`not_text`] has only one
    /// non-text unit to offer per platform, and the property under test is that
    /// the surrounding text is not flattened along with it.
    #[test]
    fn two_commands_differing_around_a_non_text_unit_do_not_collapse_together() {
        let a = parse_sudo_args(&[not_text("tool", "a")]).unwrap();
        let b = parse_sudo_args(&[not_text("tool", "b")]).unwrap();
        assert_ne!(a.command, b.command);
    }

    /// The unix-only sharpening of the case above: two commands differing
    /// *only* in the non-text byte itself, which is the shape a caller would
    /// actually use to smuggle one command past authorisation for another.
    #[cfg(unix)]
    #[test]
    fn two_commands_differing_only_in_a_high_byte_do_not_collapse_together() {
        let a = parse_sudo_args(&argv_bytes(&[b"tool\xfe"])).unwrap();
        let b = parse_sudo_args(&argv_bytes(&[b"tool\xff"])).unwrap();
        assert_ne!(a.command, b.command);
    }

    /// What follows `--` is a command, not options, whatever it contains.
    #[test]
    fn everything_after_a_double_dash_is_command_even_when_it_is_not_text() {
        let tail = not_text("arg", "");
        let args = vec![OsString::from("--"), OsString::from("-k"), tail.clone()];
        let opts = parse_sudo_args(&args).unwrap();
        assert!(!opts.invalidate, "-k after -- is an argument, not a flag");
        assert_eq!(opts.command, vec![OsString::from("-k"), tail]);
    }

    /// `-u` names a user, and a user name is looked up in `/etc/users.yaml`,
    /// which is text. A name that is not text cannot match any record there, so
    /// the parser refuses it at the boundary and says why, rather than carrying
    /// it inward to fail as "no such user" three layers down. See [`SudoOpts`].
    #[test]
    fn a_value_flag_refuses_an_argument_that_is_not_text() {
        let args = vec![
            OsString::from("-u"),
            not_text("oper", "ator"),
            OsString::from("ls"),
        ];
        let err = parse_sudo_args(&args).expect_err("a non-text user name is refused");
        let SudoError::UsageError(msg) = err else {
            panic!("expected a usage error, got {err:?}");
        };
        assert!(msg.contains("not valid text"), "unhelpful message: {msg}");
    }

    /// A file to edit is a path, not a name, so `visudo -f` takes it as it came.
    #[test]
    fn visudo_takes_the_file_to_edit_as_bytes() {
        let wanted = not_text("/etc/sudoers.d/", "");
        let opts = parse_visudo_args(&[OsString::from("-f"), wanted.clone()]).unwrap();
        assert_eq!(opts.file, wanted);
    }

    /// Likewise the I/O-log directory and the session id, which are both
    /// directory names.
    #[test]
    fn sudoreplay_takes_the_directory_and_session_as_bytes() {
        let dir = not_text("/var/log/io", "");
        let sess = not_text("sess", "");
        let opts =
            parse_sudoreplay_args(&[OsString::from("-d"), dir.clone(), sess.clone()]).unwrap();
        assert_eq!(opts.directory, dir);
        assert_eq!(opts.session_id, Some(sess));
    }

    /// argv[0] chooses the personality, and it is a path like any other. A
    /// directory component that is not text must not stop `visudo` being
    /// `visudo` — the basename is the only part that decides.
    #[test]
    fn the_personality_is_chosen_from_a_basename_that_need_not_be_text() {
        assert_eq!(
            detect_personality(not_text("/usr/sbin/", "/visudo")),
            Personality::Visudo
        );
        assert_eq!(
            detect_personality(not_text("/opt/", "/sudoedit.exe")),
            Personality::Sudoedit
        );
    }

    // -- The audit log is one record per line, and stays that way --

    /// The forging case. Before the fields were escaped, a command containing a
    /// newline appended a *second* line to `/var/log/sudo.log`, so anyone able
    /// to run `sudo` at all could write a fabricated `RESULT=ALLOWED` record
    /// naming another user into the file whose entire purpose is to say who ran
    /// what.
    #[test]
    fn a_newline_in_the_command_cannot_forge_a_second_record() {
        let forged = "ls\n2026-01-01 00:00:00 : root : TTY=x ; PWD=/ ; USER=root ; \
                      COMMAND=/bin/sh ; RESULT=ALLOWED";
        let record = format_log_record(
            "2026-09-06 12:00:00",
            "mallory",
            OsStr::new("/dev/pts/0"),
            OsStr::new("/home/mallory"),
            "root",
            OsStr::new(forged),
            "NOT_ALLOWED",
        );
        assert_eq!(record.matches('\n').count(), 1, "record: {record:?}");
        assert!(record.ends_with('\n'));
        assert!(record.contains("RESULT=NOT_ALLOWED\n"));
        assert!(
            !record.contains("RESULT=ALLOWED\n"),
            "the forged verdict became a record of its own: {record:?}"
        );
    }

    /// `$TTY` is an environment variable the caller sets outright, and the
    /// working directory is one `mkdir` plus one `cd` away — path names here
    /// may hold every byte but `/` and NUL. Both are as forgeable as argv.
    #[test]
    fn neither_the_tty_nor_the_working_directory_can_forge_a_record() {
        let record = format_log_record(
            "2026-09-06 12:00:00",
            "mallory",
            OsStr::new("x\nRESULT=ALLOWED"),
            &os_from_bytes(b"/tmp/\n\xffdir"),
            "root",
            OsStr::new("/bin/ls"),
            "NOT_ALLOWED",
        );
        assert_eq!(record.matches('\n').count(), 1, "record: {record:?}");
        assert!(record.ends_with('\n'));
    }

    /// A field that was already plain text must come through unchanged, or the
    /// escaping would have made every ordinary log line harder to read in order
    /// to defend against the rare one.
    #[test]
    fn an_ordinary_record_is_not_disturbed_by_the_escaping() {
        let record = format_log_record(
            "2026-09-06 12:00:00",
            "alice",
            OsStr::new("/dev/pts/2"),
            OsStr::new("/home/alice"),
            "root",
            OsStr::new("/usr/bin/apt install curl"),
            "SUCCESS",
        );
        assert_eq!(
            record,
            "2026-09-06 12:00:00 : alice : TTY=/dev/pts/2 ; PWD=/home/alice ; \
             USER=root ; COMMAND=/usr/bin/apt install curl ; RESULT=SUCCESS\n"
        );
    }

    // The cases below are the ones the single option table exists for: each is a
    // place where the bundled form and the standalone form previously ran through
    // separate code that had to agree by hand.

    #[test]
    fn a_value_glued_to_its_flag_reaches_the_same_field_as_a_separate_one() {
        let glued = parse_sudo_args(&argv(&["-uoperator"])).unwrap();
        let separate = parse_sudo_args(&argv(&["-u", "operator"])).unwrap();
        assert_eq!(glued.target_user, "operator");
        assert_eq!(glued.target_user, separate.target_user);
    }

    #[test]
    fn a_value_flag_ending_a_bundle_takes_the_next_argument() {
        let args = argv(&["-nu", "operator", "id"]);
        let opts = parse_sudo_args(&args).unwrap();
        assert!(opts.non_interactive);
        assert_eq!(opts.target_user, "operator");
        assert_eq!(opts.command, vec!["id"]);
    }

    #[test]
    fn a_value_flag_mid_bundle_swallows_the_rest_of_it() {
        // `-nuoperator` is `-n` plus `-u operator`, not `-n -u -o -p -e ...`.
        let opts = parse_sudo_args(&argv(&["-nuoperator"])).unwrap();
        assert!(opts.non_interactive);
        assert_eq!(opts.target_user, "operator");
    }

    #[test]
    fn a_bundle_missing_its_trailing_value_is_an_error() {
        assert!(parse_sudo_args(&argv(&["-nu"])).is_err());
    }

    #[test]
    fn a_bare_dash_and_a_long_option_are_both_rejected() {
        assert!(parse_sudo_args(&argv(&["-"])).is_err());
        assert!(parse_sudo_args(&argv(&["--frobnicate"])).is_err());
    }

    #[test]
    fn an_option_after_the_command_belongs_to_the_command() {
        let args = argv(&["id", "-u"]);
        let opts = parse_sudo_args(&args).unwrap();
        assert_eq!(opts.command, vec!["id", "-u"]);
        assert_eq!(opts.target_user, "root");
    }

    // -- Visudo option parsing tests --

    #[test]
    fn parse_visudo_defaults() {
        let args: Vec<OsString> = vec![];
        let opts = parse_visudo_args(&args).unwrap();
        assert!(!opts.check_only);
        assert_eq!(opts.file, SUDOERS_PATH);
        assert!(!opts.strict);
    }

    #[test]
    fn parse_visudo_check_only() {
        let args = argv(&["-c"]);
        let opts = parse_visudo_args(&args).unwrap();
        assert!(opts.check_only);
    }

    #[test]
    fn parse_visudo_alternate_file() {
        let args = argv(&["-f", "/tmp/sudoers"]);
        let opts = parse_visudo_args(&args).unwrap();
        assert_eq!(opts.file, "/tmp/sudoers");
    }

    #[test]
    fn parse_visudo_strict() {
        let args = argv(&["-s"]);
        let opts = parse_visudo_args(&args).unwrap();
        assert!(opts.strict);
    }

    #[test]
    fn parse_visudo_unknown_flag() {
        let args = argv(&["-z"]);
        assert!(parse_visudo_args(&args).is_err());
    }

    #[test]
    fn parse_visudo_f_missing_value() {
        let args = argv(&["-f"]);
        assert!(parse_visudo_args(&args).is_err());
    }

    // -- Sudoreplay option parsing tests --

    #[test]
    fn parse_sudoreplay_defaults() {
        let args: Vec<OsString> = vec![];
        let opts = parse_sudoreplay_args(&args).unwrap();
        assert!(!opts.list);
        assert_eq!(opts.directory, SUDO_IO_DIR);
        assert!((opts.speed_factor - 1.0).abs() < f64::EPSILON);
        assert!(opts.session_id.is_none());
    }

    #[test]
    fn parse_sudoreplay_list() {
        let args = argv(&["-l"]);
        let opts = parse_sudoreplay_args(&args).unwrap();
        assert!(opts.list);
    }

    #[test]
    fn parse_sudoreplay_directory() {
        let args = argv(&["-d", "/tmp/logs"]);
        let opts = parse_sudoreplay_args(&args).unwrap();
        assert_eq!(opts.directory, "/tmp/logs");
    }

    #[test]
    fn parse_sudoreplay_speed() {
        let args = argv(&["-s", "2.5"]);
        let opts = parse_sudoreplay_args(&args).unwrap();
        assert!((opts.speed_factor - 2.5).abs() < f64::EPSILON);
    }

    #[test]
    fn parse_sudoreplay_session_id() {
        let args = argv(&["abc123"]);
        let opts = parse_sudoreplay_args(&args).unwrap();
        assert_eq!(opts.session_id.as_deref(), Some(OsStr::new("abc123")));
    }

    #[test]
    fn parse_sudoreplay_negative_speed() {
        let args = argv(&["-s", "-1"]);
        assert!(parse_sudoreplay_args(&args).is_err());
    }

    #[test]
    fn parse_sudoreplay_zero_speed() {
        let args = argv(&["-s", "0"]);
        assert!(parse_sudoreplay_args(&args).is_err());
    }

    #[test]
    fn parse_sudoreplay_invalid_speed() {
        let args = argv(&["-s", "notanumber"]);
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
        for content in [
            "alice ALL = NOPASSWD:\n",
            "alice ALL = NOPASSWORD: /bin/ls\n",
        ] {
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
        let mut env: Vec<(OsString, OsString)> = vec![];
        set_or_replace(&mut env, "KEY", OsStr::new("val"));
        assert_eq!(env.len(), 1);
        assert_eq!(env[0], (OsString::from("KEY"), OsString::from("val")));
    }

    #[test]
    fn set_or_replace_existing() {
        let mut env = vec![(OsString::from("KEY"), OsString::from("old"))];
        set_or_replace(&mut env, "KEY", OsStr::new("new"));
        assert_eq!(env.len(), 1);
        assert_eq!(env[0].1, "new");
    }

    /// A variable's *value* is bytes: `PATH` and `HOME` are paths, and a path
    /// here may hold every byte but `/` and NUL. The value must reach the child
    /// process as the bytes it is, not as whatever survives a UTF-8 check.
    #[test]
    fn set_or_replace_carries_a_value_that_is_not_text() {
        let mut env: Vec<(OsString, OsString)> = vec![];
        let weird = not_text("/home/", "");
        set_or_replace(&mut env, "HOME", &weird);
        assert_eq!(env[0].1, weird);
    }

    /// The byte-exact form, on the platform where an `OsStr` is its bytes.
    /// `HOME` is handed to the child through `execve`'s environment as bytes;
    /// one substituted byte is a different directory.
    #[cfg(unix)]
    #[test]
    fn set_or_replace_carries_a_value_byte_for_byte() {
        let mut env: Vec<(OsString, OsString)> = vec![];
        let weird = os_from_bytes(b"/home/\xff");
        set_or_replace(&mut env, "HOME", &weird);
        assert_eq!(&*os_bytes(&env[0].1), b"/home/\xff");
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
        let args = argv(&["-iubob", "ls"]);
        let opts = parse_sudo_args(&args).unwrap();
        assert!(opts.login_shell);
        assert_eq!(opts.target_user, "bob");
        assert_eq!(opts.command, vec!["ls"]);
    }

    #[test]
    fn parse_sudo_combined_flags_with_group() {
        let args = argv(&["-gstaff", "ls"]);
        let opts = parse_sudo_args(&args).unwrap();
        assert_eq!(opts.target_group, "staff");
    }

    #[test]
    fn parse_sudo_combined_flags_with_prompt() {
        let args = argv(&["-pEnter:", "ls"]);
        let opts = parse_sudo_args(&args).unwrap();
        assert_eq!(opts.prompt, "Enter:");
    }

    // -- sudoers' authorization rules (2026-10-01) --
    //
    // Each of these failed before that day's fix: arguments were ignored, a
    // negation granted everything but what it named, a pattern was a prefix
    // test on the path as typed, sudoedit asked about running the file, and
    // shell mode authorised the first word of a line handed to `sh -c`.

    /// Shorthand: may alice run `command` with `args` under `sudoers`?
    fn alice_may(sudoers: &str, command: &[u8], args: Option<&[u8]>) -> bool {
        let config = parse_sudoers(sudoers).expect("sudoers parses");
        check_authorization(
            &config,
            "alice",
            "localhost",
            "root",
            "",
            &Request { command, args },
            &["alice".to_string()],
        )
        .is_some()
    }

    #[test]
    fn a_rules_arguments_restrict_it() {
        let rule = "alice ALL = (root) /usr/bin/systemctl restart nginx\n";
        assert!(alice_may(
            rule,
            b"/usr/bin/systemctl",
            Some(b"restart nginx")
        ));
        assert!(!alice_may(rule, b"/usr/bin/systemctl", Some(b"stop nginx")));
        assert!(!alice_may(
            rule,
            b"/usr/bin/systemctl",
            Some(b"restart nginx sshd")
        ));
        assert!(!alice_may(rule, b"/usr/bin/systemctl", None));
    }

    #[test]
    fn empty_quotes_allow_no_arguments() {
        let rule = "alice ALL = (root) /usr/bin/id \"\"\n";
        assert!(alice_may(rule, b"/usr/bin/id", None));
        assert!(!alice_may(rule, b"/usr/bin/id", Some(b"-u")));
    }

    #[test]
    fn a_rule_without_arguments_allows_any() {
        let rule = "alice ALL = (root) /usr/bin/id\n";
        assert!(alice_may(rule, b"/usr/bin/id", None));
        assert!(alice_may(rule, b"/usr/bin/id", Some(b"-u root")));
    }

    #[test]
    fn rule_arguments_are_fnmatch_patterns() {
        let rule = "alice ALL = (root) /usr/bin/systemctl restart *\n";
        assert!(alice_may(
            rule,
            b"/usr/bin/systemctl",
            Some(b"restart nginx")
        ));
        // Not a sudoedit rule, so no FNM_PATHNAME: `*` crosses a `/` here,
        // as it does upstream.
        assert!(alice_may(rule, b"/usr/bin/systemctl", Some(b"restart a/b")));
        assert!(!alice_may(rule, b"/usr/bin/systemctl", Some(b"stop nginx")));
    }

    #[test]
    fn rule_arguments_between_caret_and_dollar_are_a_regex() {
        let rule = "alice ALL = (root) /usr/bin/systemctl ^restart (nginx|apache2)$\n";
        assert!(alice_may(
            rule,
            b"/usr/bin/systemctl",
            Some(b"restart nginx")
        ));
        assert!(alice_may(
            rule,
            b"/usr/bin/systemctl",
            Some(b"restart apache2")
        ));
        assert!(!alice_may(
            rule,
            b"/usr/bin/systemctl",
            Some(b"restart sshd")
        ));
        // A pattern that does not compile grants nothing.
        assert!(!alice_may(
            "alice ALL = (root) /usr/bin/x ^(unclosed$\n",
            b"/usr/bin/x",
            Some(b"(unclosed")
        ));
    }

    #[test]
    fn a_negated_command_is_refused_over_all() {
        let rule = "alice ALL = (root) ALL, !/usr/bin/passwd\n";
        assert!(!alice_may(rule, b"/usr/bin/passwd", None));
        assert!(alice_may(rule, b"/usr/bin/id", None));
        // White space after the `!` is allowed, as sudoers allows it.
        let spaced = "alice ALL = (root) ALL, ! /usr/bin/passwd\n";
        assert!(!alice_may(spaced, b"/usr/bin/passwd", None));
        assert!(alice_may(spaced, b"/usr/bin/id", None));
    }

    #[test]
    fn a_negation_alone_grants_nothing() {
        let rule = "alice ALL = (root) !/usr/bin/passwd\n";
        assert!(!alice_may(rule, b"/usr/bin/id", None));
        assert!(!alice_may(rule, b"/usr/bin/passwd", None));
    }

    #[test]
    fn a_negated_alias_refuses_its_members() {
        let rule = "Cmnd_Alias SHELLS = /bin/sh, /bin/bash\n\
                    alice ALL = (root) ALL, !SHELLS\n";
        assert!(!alice_may(rule, b"/bin/sh", None));
        assert!(!alice_may(rule, b"/bin/bash", None));
        assert!(alice_may(rule, b"/usr/bin/id", None));
    }

    /// A later privilege decides over an earlier one, as a later line of
    /// sudoers does.
    #[test]
    fn the_last_rule_that_names_the_command_decides() {
        let allow_last = "alice ALL = (root) !/usr/bin/passwd\nalice ALL = (root) ALL\n";
        assert!(alice_may(allow_last, b"/usr/bin/passwd", None));
        let deny_last = "alice ALL = (root) ALL\nalice ALL = (root) !/usr/bin/passwd\n";
        assert!(!alice_may(deny_last, b"/usr/bin/passwd", None));
    }

    #[test]
    fn sudoedit_is_its_own_command() {
        let rule = "alice ALL = (root) sudoedit /etc/motd\n";
        assert!(alice_may(rule, b"sudoedit", Some(b"/etc/motd")));
        assert!(!alice_may(rule, b"sudoedit", Some(b"/etc/shadow")));
        // Several files are one argument string, which the rule must match.
        assert!(!alice_may(
            rule,
            b"sudoedit",
            Some(b"/etc/motd /etc/shadow")
        ));
        // A rule to RUN a path does not grant editing it.
        assert!(!alice_may(
            "alice ALL = (root) /etc/motd\n",
            b"sudoedit",
            Some(b"/etc/motd")
        ));
        // A sudoedit rule runs nothing.
        assert!(!alice_may(rule, b"/usr/bin/sudoedit", Some(b"/etc/motd")));
        // `ALL` includes sudoedit.
        assert!(alice_may(
            "alice ALL = (root) ALL\n",
            b"sudoedit",
            Some(b"/etc/shadow")
        ));
    }

    #[test]
    fn sudoedit_patterns_do_not_cross_a_slash() {
        let rule = "alice ALL = (root) sudoedit /etc/*\n";
        assert!(alice_may(rule, b"sudoedit", Some(b"/etc/motd")));
        assert!(!alice_may(rule, b"sudoedit", Some(b"/etc/ssh/sshd_config")));
    }

    /// None of these exists, so each is judged by its text -- and a text with
    /// `.`, `..` or an empty component is not a path a pattern may match.
    #[test]
    fn a_pattern_does_not_match_through_dot_dot() {
        let rule = "alice ALL = (root) /usr/bin/*\n";
        assert!(!alice_may(rule, b"/usr/bin/../../tmp/evil", None));
        assert!(!alice_may(rule, b"/usr/bin/./../sbin/x", None));
        assert!(!alice_may(rule, b"/usr/bin//x", None));
        assert!(!alice_may(rule, b"/usr/bin/sub/x", None));
        assert!(alice_may(rule, b"/usr/bin/nonexistent-tool", None));
    }

    /// With real files the canonical program is what a pattern sees.
    #[cfg(unix)]
    #[test]
    fn a_pattern_is_matched_against_the_canonical_program() {
        let dir = ScratchDir::new("sudo_canon");
        let root = fs::canonicalize(dir.dir())
            .expect("canonical scratch dir")
            .to_string_lossy()
            .to_string();
        fs::create_dir_all(format!("{root}/bin")).expect("bin");
        fs::write(format!("{root}/bin/tool"), b"x").expect("tool");
        fs::write(format!("{root}/evil"), b"x").expect("evil");

        let rule = format!("alice ALL = (root) {root}/bin/*\n");
        assert!(alice_may(
            &rule,
            format!("{root}/bin/tool").as_bytes(),
            None
        ));
        assert!(!alice_may(
            &rule,
            format!("{root}/bin/../evil").as_bytes(),
            None
        ));

        // A directory rule: every program directly in it.
        let dir_rule = format!("alice ALL = (root) {root}/bin/\n");
        assert!(alice_may(
            &dir_rule,
            format!("{root}/bin/tool").as_bytes(),
            None
        ));
        assert!(!alice_may(
            &dir_rule,
            format!("{root}/evil").as_bytes(),
            None
        ));
        assert!(!alice_may(
            &dir_rule,
            format!("{root}/bin/../evil").as_bytes(),
            None
        ));
    }

    #[test]
    fn shell_mode_runs_the_shell_with_the_words_escaped() {
        let opts = SudoOpts {
            shell: true,
            command: vec![OsString::from("ls"), OsString::from("&& id")],
            ..SudoOpts::default()
        };
        assert_eq!(
            invocation(&opts, "/bin/sh"),
            Some((
                OsString::from("/bin/sh"),
                vec![OsString::from("-c"), OsString::from("ls \\&\\&\\ id")]
            ))
        );
        // Alone, the shell itself.
        let alone = SudoOpts {
            login_shell: true,
            ..SudoOpts::default()
        };
        assert_eq!(
            invocation(&alone, "/bin/sh"),
            Some((OsString::from("/bin/sh"), Vec::new()))
        );
    }

    #[test]
    fn without_a_shell_the_command_line_is_the_invocation() {
        let opts = SudoOpts {
            command: vec![OsString::from("ls"), OsString::from("-l")],
            ..SudoOpts::default()
        };
        assert_eq!(
            invocation(&opts, "/bin/sh"),
            Some((OsString::from("ls"), vec![OsString::from("-l")]))
        );
        assert_eq!(invocation(&SudoOpts::default(), "/bin/sh"), None);
    }

    #[test]
    fn shell_escaping_is_upstreams() {
        let words = |w: &[&str]| -> Vec<OsString> { w.iter().map(OsString::from).collect() };
        assert_eq!(
            shell_escaped_command(&words(&["a$b_c-d9"])),
            OsString::from("a$b_c-d9")
        );
        assert_eq!(
            shell_escaped_command(&words(&["x y", "z"])),
            OsString::from("x\\ y z")
        );
        assert_eq!(
            shell_escaped_command(&words(&["a;b", "'c'"])),
            OsString::from("a\\;b \\'c\\'")
        );
    }

    /// In shell mode the SHELL is what sudoers is asked about, so a caller
    /// allowed one program cannot reach the shell through it.
    #[test]
    fn shell_mode_authorises_the_shell_not_the_first_word() {
        let opts = SudoOpts {
            shell: true,
            command: vec![OsString::from("/usr/bin/id"), OsString::from("&& reboot")],
            ..SudoOpts::default()
        };
        let (program, args) = invocation(&opts, "/bin/sh").expect("something to run");
        let program = os_bytes(&program).into_owned();
        let joined = user_args(&args);
        let config = parse_sudoers("alice ALL = (root) /usr/bin/id\n").expect("parses");
        let request = Request {
            command: &program,
            args: joined.as_deref(),
        };
        assert!(
            check_authorization(
                &config,
                "alice",
                "localhost",
                "root",
                "",
                &request,
                &["alice".to_string()]
            )
            .is_none()
        );
    }

    #[test]
    fn user_args_are_joined_with_single_spaces() {
        assert_eq!(user_args(&[]), None);
        assert_eq!(
            user_args(&[OsString::from("a"), OsString::from("b c")]),
            Some(b"a b c".to_vec())
        );
        assert_eq!(user_args(&[OsString::new()]), Some(Vec::new()));
    }

    // -- sudoedit: the editor runs as the caller on copies the caller owns --

    #[test]
    fn a_copy_is_named_as_upstream_names_it() {
        assert_eq!(temp_name(b"motd.conf", b"ABCDEFGH"), b"motdABCDEFGH.conf");
        assert_eq!(temp_name(b"motd", b"ABCDEFGH"), b"motd.ABCDEFGH");
        assert_eq!(temp_name(b".bashrc", b"ABCDEFGH"), b"ABCDEFGH.bashrc");
        assert_eq!(temp_name(b"a.b.c", b"ABCDEFGH"), b"a.bABCDEFGH.c");
    }

    #[test]
    fn a_directory_is_writable_as_upstream_judges_it() {
        let alice = Caller {
            uid: 1000,
            gid: 1000,
        };
        // The caller's own directory is, whatever its mode.
        assert!(dir_writable_by(0o555, 1000, 0, alice));
        // Others may write: so may the caller, sticky bit or not.
        assert!(dir_writable_by(0o777, 0, 0, alice));
        assert!(dir_writable_by(0o1777, 0, 0, alice));
        // The group may write, and it is the caller's group.
        assert!(dir_writable_by(0o775, 0, 1000, alice));
        assert!(!dir_writable_by(0o775, 0, 50, alice));
        // Root's ordinary directory is not.
        assert!(!dir_writable_by(0o755, 0, 0, alice));
    }

    #[test]
    fn the_editor_setting_is_split_into_words() {
        assert_eq!(
            editor_words(OsStr::new("vim -n")),
            vec![OsString::from("vim"), OsString::from("-n")]
        );
        assert_eq!(
            editor_words(OsStr::new("  ")),
            vec![OsString::from(DEFAULT_EDITOR)]
        );
    }

    #[cfg(unix)]
    #[test]
    fn the_open_flags_are_the_c_librarys() {
        assert_eq!(O_NOFOLLOW, posix::fcntl::O_NOFOLLOW);
        assert_eq!(O_NONBLOCK, posix::fcntl::O_NONBLOCK);
        assert_eq!(ELOOP, posix::errno::ELOOP);
    }

    /// The test's own identity, as sudoedit would see its caller.
    #[cfg(unix)]
    fn me() -> Caller {
        Caller {
            uid: authlib::identity::caller_uid().expect("uid"),
            gid: authlib::identity::caller_gid().expect("gid"),
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_file_in_a_directory_the_caller_can_write_is_refused() {
        let dir = ScratchDir::new("sudoedit_dirs");
        let file = dir.dir().join("notes.txt");
        fs::write(&file, b"x").expect("write");
        let refused = check_path_dirs(&file, me()).expect_err("own directory");
        assert!(
            refused.ends_with("editing files in a writable directory is not permitted"),
            "{refused}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_is_not_opened() {
        let dir = ScratchDir::new("sudoedit_link");
        let target = dir.dir().join("target");
        fs::write(&target, b"secret").expect("write");
        let link = dir.dir().join("link");
        std::os::unix::fs::symlink(&target, &link).expect("symlink");
        let refused = open_original(&link).expect_err("a symlink");
        assert!(
            refused.ends_with("editing symbolic links is not permitted"),
            "{refused}"
        );
        // And a directory in the path that is a symlink is refused too. Asked
        // of that one step: walked from the root, this path fails earlier, at
        // the world-writable `/tmp` it sits in -- which is also right.
        let linked_dir = dir.dir().join("linked");
        std::os::unix::fs::symlink(dir.dir(), &linked_dir).expect("symlink dir");
        let refused = check_dir(
            &linked_dir,
            "'linked/target'",
            Caller {
                uid: u32::MAX,
                gid: u32::MAX,
            },
        )
        .expect_err("a symlinked directory");
        assert!(
            refused.ends_with("editing symbolic links is not permitted"),
            "{refused}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_missing_original_is_edited_from_empty_and_a_directory_is_refused() {
        let dir = ScratchDir::new("sudoedit_open");
        assert!(matches!(open_original(&dir.dir().join("absent")), Ok(None)));
        let refused = open_original(dir.dir()).expect_err("a directory");
        assert!(refused.ends_with("not a regular file"), "{refused}");
    }

    /// A copy made, edited and written back, without the editor: the two
    /// halves either side of it, which are where the checks are.
    #[cfg(unix)]
    #[test]
    fn a_copy_round_trips_and_an_untouched_one_is_unchanged() {
        let dir = ScratchDir::new("sudoedit_round");
        let original = dir.dir().join("motd");
        fs::write(&original, b"hello\n").expect("write");
        let tmpdir = dir.dir().join("tmp");
        fs::create_dir(&tmpdir).expect("tmp");

        // The copy: exclusive, 0600, the caller's, holding the original.
        let edit = prepare_copy(&original, &tmpdir, me());
        assert_eq!(fs::read(&edit.temp).expect("read copy"), b"hello\n");
        {
            use std::os::unix::fs::MetadataExt as _;
            let meta = fs::metadata(&edit.temp).expect("meta");
            assert_eq!(meta.mode() & 0o7777, 0o600);
            assert_eq!(meta.uid(), me().uid);
        }

        // Untouched: reported unchanged, the copy removed, the original kept.
        assert_eq!(copy_back(&edit, me(), true), Ok(()));
        assert!(!edit.temp.exists());
        assert_eq!(fs::read(&original).expect("read"), b"hello\n");

        // Edited shorter: written back, and cut at the new length.
        let edit = prepare_copy(&original, &tmpdir, me());
        fs::write(&edit.temp, b"hi\n").expect("edit");
        assert_eq!(copy_back(&edit, me(), true), Ok(()));
        assert_eq!(fs::read(&original).expect("read"), b"hi\n");
        assert!(!edit.temp.exists());
    }

    /// The checks a copy must pass before it is trusted over the original.
    #[cfg(unix)]
    #[test]
    fn a_copy_that_is_not_what_was_made_is_not_written_back() {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = ScratchDir::new("sudoedit_swap");
        let original = dir.dir().join("motd");
        fs::write(&original, b"hello\n").expect("write");
        let tmpdir = dir.dir().join("tmp");
        fs::create_dir(&tmpdir).expect("tmp");
        let secret = dir.dir().join("secret");
        fs::write(&secret, b"root's\n").expect("secret");

        // Swapped for a symlink: refused, the original left alone.
        let edit = prepare_copy(&original, &tmpdir, me());
        fs::remove_file(&edit.temp).expect("rm");
        std::os::unix::fs::symlink(&secret, &edit.temp).expect("symlink");
        assert!(copy_back(&edit, me(), true).is_err());
        assert_eq!(fs::read(&original).expect("read"), b"hello\n");

        // Its mode changed: refused.
        let edit = prepare_copy(&original, &tmpdir, me());
        fs::set_permissions(&edit.temp, fs::Permissions::from_mode(0o644)).expect("chmod");
        assert!(copy_back(&edit, me(), true).is_err());

        // Owned by someone else (as seen by a different caller): refused.
        let edit = prepare_copy(&original, &tmpdir, me());
        let stranger = Caller {
            uid: me().uid.wrapping_add(1),
            gid: me().gid,
        };
        assert!(copy_back(&edit, stranger, true).is_err());
        assert_eq!(fs::read(&original).expect("read"), b"hello\n");
    }

    /// `prepare_edit` without the directory check, which a test cannot pass:
    /// every directory it can create is its own.
    #[cfg(unix)]
    fn prepare_copy(original: &Path, tmpdir: &Path, caller: Caller) -> EditFile {
        prepare_edit_unchecked(original, tmpdir, caller).expect("copy made")
    }

    // -- visudo: lock, copy beside the file, install by rename --

    #[test]
    fn what_now_takes_upstreams_three_answers() {
        assert_eq!(what_now("e\n"), Some(WhatNow::Edit));
        assert_eq!(what_now("x"), Some(WhatNow::Exit));
        assert_eq!(what_now("Q\n"), Some(WhatNow::Quit));
        // Anything else is asked again; `q` is not `Q`, as upstream has it.
        assert_eq!(what_now("q"), None);
        assert_eq!(what_now(""), None);
    }

    #[test]
    fn the_copy_sits_beside_the_file() {
        assert_eq!(
            sudoers_temp_path(Path::new("/etc/sudoers")),
            PathBuf::from("/etc/sudoers.tmp")
        );
    }

    #[test]
    fn the_copy_ends_in_a_newline() {
        assert_eq!(sudoers_starting_text(b"a\nb"), b"a\nb\n");
        assert_eq!(sudoers_starting_text(b"a\n"), b"a\n");
        assert_eq!(sudoers_starting_text(b""), b"");
    }

    /// An "editor" that replaces the copy's contents: `sh -c SCRIPT sh -- FILE`,
    /// so the copy is `$2`.
    #[cfg(unix)]
    fn writes(text: &str) -> Vec<OsString> {
        vec![
            OsString::from("sh"),
            OsString::from("-c"),
            OsString::from(format!("printf '%s' '{text}' > \"$2\"")),
            OsString::from("sh"),
        ]
    }

    #[cfg(unix)]
    #[test]
    fn a_held_lock_is_busy() {
        let dir = ScratchDir::new("visudo_lock");
        let path = dir.dir().join("sudoers");
        fs::write(&path, b"root ALL = (ALL) ALL\n").expect("write");
        let held = fs::File::open(&path).expect("open");
        held.lock().expect("lock");
        assert_eq!(edit_sudoers(&path, false, &writes("x")), 1);
        // Nothing was copied or changed while it was busy.
        assert!(!sudoers_temp_path(&path).exists());
        assert_eq!(fs::read(&path).expect("read"), b"root ALL = (ALL) ALL\n");
    }

    #[cfg(unix)]
    #[test]
    fn a_good_edit_is_installed_read_only_by_rename() {
        use std::os::unix::fs::MetadataExt as _;
        let dir = ScratchDir::new("visudo_install");
        let path = dir.dir().join("sudoers");
        fs::write(&path, b"root ALL = (ALL) ALL\n").expect("write");
        let wanted = "root ALL = (ALL) ALL\nalice ALL = (root) /usr/bin/id\n";
        assert_eq!(edit_sudoers(&path, false, &writes(wanted)), 0);
        assert_eq!(fs::read_to_string(&path).expect("read"), wanted);
        assert_eq!(fs::metadata(&path).expect("meta").mode() & 0o777, 0o440);
        assert!(!sudoers_temp_path(&path).exists());
    }

    #[cfg(unix)]
    #[test]
    fn an_untouched_copy_installs_nothing() {
        let dir = ScratchDir::new("visudo_untouched");
        let path = dir.dir().join("sudoers");
        fs::write(&path, b"root ALL = (ALL) ALL\n").expect("write");
        let editor = vec![OsString::from("true")];
        assert_eq!(edit_sudoers(&path, false, &editor), 0);
        assert_eq!(fs::read(&path).expect("read"), b"root ALL = (ALL) ALL\n");
        assert!(!sudoers_temp_path(&path).exists());
    }

    #[cfg(unix)]
    #[test]
    fn an_emptied_copy_is_refused() {
        let dir = ScratchDir::new("visudo_empty");
        let path = dir.dir().join("sudoers");
        fs::write(&path, b"root ALL = (ALL) ALL\n").expect("write");
        assert_eq!(edit_sudoers(&path, false, &writes("")), 1);
        assert_eq!(fs::read(&path).expect("read"), b"root ALL = (ALL) ALL\n");
        assert!(!sudoers_temp_path(&path).exists());
    }

    /// The copy is opened without following a symlink, so one planted at its
    /// name cannot aim the edit at another file.
    #[cfg(unix)]
    #[test]
    fn a_symlink_at_the_copys_name_is_not_followed() {
        let dir = ScratchDir::new("visudo_link");
        let path = dir.dir().join("sudoers");
        fs::write(&path, b"root ALL = (ALL) ALL\n").expect("write");
        let victim = dir.dir().join("victim");
        fs::write(&victim, b"untouched\n").expect("victim");
        std::os::unix::fs::symlink(&victim, sudoers_temp_path(&path)).expect("plant");
        assert_eq!(edit_sudoers(&path, false, &writes("x")), 1);
        assert_eq!(fs::read(&victim).expect("read"), b"untouched\n");
        assert_eq!(fs::read(&path).expect("read"), b"root ALL = (ALL) ALL\n");
    }
}
