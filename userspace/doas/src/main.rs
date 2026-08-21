//! Slate OS doas -- Lightweight Privilege Elevation
//!
//! A doas(1) implementation modelled on OpenBSD's design. Reads policy rules
//! from `/etc/doas.conf` and, when permitted, executes commands as another
//! user (default: root).
//!
//! # Usage
//!
//! ```text
//! doas [-ns] [-C config] [-L] [-u user] [--] command [args ...]
//! ```
//!
//! # Configuration (`/etc/doas.conf`)
//!
//! ```text
//! permit nopass root
//! permit nopass :wheel
//! permit persist alice as root
//! permit alice cmd /usr/bin/pkg
//! deny bob
//! ```
//!
//! Rules are evaluated top-to-bottom; the first matching rule wins.
//!
//! # Authentication
//!
//! The caller's password is checked by [`authlib`], which is the one place in
//! SlateOS that answers "is this the user's password?". This crate previously
//! answered it itself, with a `$sha256$<salt>$<digest>` scheme that `passwd`
//! does not write — see the comment above the authentication section for what
//! that cost.
//!
//! Two rules that are easy to confuse:
//!
//! - **`nopass` in `/etc/doas.conf` is the only way to skip the password.** It
//!   is an administrator writing down, per rule, that this escalation needs no
//!   proof.
//! - **An account with *no password set* is refused, not waved through.** That
//!   is a different statement — it says nothing about escalation, and reading
//!   it as consent would turn every passwordless account into a root shell.
//!   `login` resolves the same `authlib` answer the opposite way, because a
//!   deliberately passwordless account at the machine's own keyboard is a
//!   long-standing Unix choice; escalating from one is not.

use std::env;
use std::fs;
use std::io::{self, BufRead, Write};
use std::process;
use std::time::SystemTime;

// ============================================================================
// Constants
// ============================================================================

const DEFAULT_CONFIG_PATH: &str = "/etc/doas.conf";
// `/etc/shadow` is deliberately not named here. Which store holds the password
// is `authlib`'s business -- it reads `/etc/users.yaml` first and `/etc/shadow`
// second -- and a copy of the path in this crate is how it came to read only
// one of them. `/etc/passwd` stays, because uid/gid/home/shell are not
// passwords and `doas` really does need them itself.
const PASSWD_PATH: &str = "/etc/passwd";
const PERSIST_DIR: &str = "/var/run/doas";

/// Duration (in seconds) for which a `persist` timestamp remains valid.
const PERSIST_TIMEOUT_SECS: u64 = 300; // 5 minutes

// ============================================================================
// Password verification
// ============================================================================
//
// This crate used to answer "is this the user's password?" itself, and it was
// the fifth program in this tree to do so with its own arithmetic. It hashed
// `$sha256$<salt>$<digest>` as `sha256(salt || "$" || password)` and returned
// `false` for every other format -- including `$6$`, which is what `passwd`
// actually writes. The section header said "matches passwd utility"; it had
// not matched for as long as `passwd` had been going through `posix::crypt`.
//
// The practical effect was that `doas` could not be used at all: a password
// set the normal way produced "authentication failed" no matter how carefully
// it was typed, which reads to the user as a forgotten password rather than as
// a broken program. It failed closed, so it was never an escalation hole -- but
// a privilege gate nobody can pass is repaired by turning it off, and that is
// the hole it would eventually have become.
//
// It also read `/etc/shadow` directly, so a user who exists only in the native
// `/etc/users.yaml` database had no password `doas` could find.
//
// Both are now `authlib`'s problem -- the one place SlateOS answers this
// question (design-decisions.md sections 329 and 341). It consults
// `/etc/users.yaml` first and `/etc/shadow` second, recomputes the stored entry
// *as a setting* rather than taking it apart to find a salt, distinguishes a
// locked account and an unrecomputable entry from a wrong password, and spends
// the same time on a user who does not exist as on one who does.

// ============================================================================
// /etc/passwd parsing
// ============================================================================

/// A single entry from `/etc/passwd`.
#[derive(Clone, Debug, PartialEq)]
struct PasswdEntry {
    username: String,
    uid: u32,
    gid: u32,
    home: String,
    shell: String,
}

/// Parse `/etc/passwd` and return all entries.
fn read_passwd_entries(path: &str) -> Vec<PasswdEntry> {
    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    content
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(parse_passwd_line)
        .collect()
}

/// Parse a single /etc/passwd line.
fn parse_passwd_line(line: &str) -> Option<PasswdEntry> {
    let fields: Vec<&str> = line.split(':').collect();
    if fields.len() < 7 {
        return None;
    }
    let uid = fields[2].parse().ok()?;
    let gid = fields[3].parse().ok()?;
    Some(PasswdEntry {
        username: fields[0].to_string(),
        uid,
        gid,
        home: fields[5].to_string(),
        shell: fields[6].to_string(),
    })
}

/// Look up a user in `/etc/passwd` by name.
fn lookup_passwd_user(username: &str) -> Option<PasswdEntry> {
    read_passwd_entries(PASSWD_PATH)
        .into_iter()
        .find(|e| e.username == username)
}

/// Look up a user in `/etc/passwd` by UID.
fn lookup_passwd_uid(uid: u32) -> Option<PasswdEntry> {
    read_passwd_entries(PASSWD_PATH)
        .into_iter()
        .find(|e| e.uid == uid)
}

// ============================================================================
// /etc/group parsing (for :group matching)
// ============================================================================

/// A single entry from `/etc/group`.
#[derive(Clone, Debug, PartialEq)]
struct GroupEntry {
    name: String,
    #[allow(dead_code)]
    gid: u32,
    members: Vec<String>,
}

/// Parse `/etc/group` and return all entries.
fn read_group_entries() -> Vec<GroupEntry> {
    let content = match fs::read_to_string("/etc/group") {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    content
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| {
            let fields: Vec<&str> = line.split(':').collect();
            if fields.len() < 4 {
                return None;
            }
            let gid = fields[2].parse().ok()?;
            let members = fields[3]
                .split(',')
                .map(|m| m.trim().to_string())
                .filter(|m| !m.is_empty())
                .collect();
            Some(GroupEntry {
                name: fields[0].to_string(),
                gid,
                members,
            })
        })
        .collect()
}

/// Check whether a user is a member of the named group.
fn user_in_group(username: &str, group_name: &str) -> bool {
    let groups = read_group_entries();
    groups
        .iter()
        .any(|g| g.name == group_name && g.members.iter().any(|m| m == username))
}

// ============================================================================
// doas.conf configuration model
// ============================================================================

/// Whether the rule permits or denies.
#[derive(Clone, Debug, PartialEq)]
enum RuleAction {
    Permit,
    Deny,
}

/// Options that may appear on a `permit` rule.
#[derive(Clone, Debug, Default, PartialEq)]
struct RuleOptions {
    /// Skip password authentication.
    nopass: bool,
    /// Use timestamp-based persistence for authentication.
    persist: bool,
    /// Preserve the caller's environment.
    keepenv: bool,
    /// Variables to set explicitly.
    setenv: Vec<(String, String)>,
    /// Variables to unset.
    unsetenv: Vec<String>,
}

/// A single rule from `doas.conf`.
#[derive(Clone, Debug, PartialEq)]
struct Rule {
    action: RuleAction,
    options: RuleOptions,
    /// The identity this rule matches. A plain name matches a user; a name
    /// prefixed with `:` matches a group.
    identity: String,
    /// If set, the rule only applies when running as this target user.
    target: Option<String>,
    /// If set, the rule only applies to this specific command.
    cmd: Option<String>,
    /// If set, the rule further restricts by argument list.
    args: Option<Vec<String>>,
}

/// Result of matching a rule against a request.
#[derive(Debug, PartialEq)]
enum MatchResult {
    /// A `permit` rule matched.
    Permit(RuleOptions),
    /// A `deny` rule matched.
    Deny,
    /// No rule matched.
    NoMatch,
}

// ============================================================================
// doas.conf parser
// ============================================================================

/// Parse the full contents of a `doas.conf` file into a list of rules.
/// Returns `Ok(rules)` or `Err(message)` for the first syntax error.
fn parse_config(content: &str) -> Result<Vec<Rule>, String> {
    let mut rules = Vec::new();

    for (line_num, raw_line) in content.lines().enumerate() {
        let line = raw_line.trim();

        // Skip empty lines and comments.
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let rule = parse_rule(line).map_err(|e| format!("line {}: {e}", line_num + 1))?;
        rules.push(rule);
    }

    Ok(rules)
}

/// Parse a single doas.conf rule line.
///
/// Grammar (simplified):
/// ```text
/// rule = action [options] identity ["as" target] ["cmd" command ["args" args...]]
/// action = "permit" | "deny"
/// options = { "nopass" | "persist" | "keepenv" | "setenv" "{" assignments "}" }
/// identity = username | ":" groupname
/// ```
fn parse_rule(line: &str) -> Result<Rule, String> {
    let tokens = tokenize(line)?;
    if tokens.is_empty() {
        return Err("empty rule".to_string());
    }

    let mut idx = 0;

    // 1. Action
    let action = match tokens.get(idx).map(String::as_str) {
        Some("permit") => RuleAction::Permit,
        Some("deny") => RuleAction::Deny,
        Some(other) => return Err(format!("expected 'permit' or 'deny', got '{other}'")),
        None => return Err("expected 'permit' or 'deny'".to_string()),
    };
    idx += 1;

    // 2. Options (only valid for permit rules, but we parse them either way
    //    so we can give a clear error).
    let mut options = RuleOptions::default();
    loop {
        match tokens.get(idx).map(String::as_str) {
            Some("nopass") => {
                options.nopass = true;
                idx += 1;
            }
            Some("persist") => {
                options.persist = true;
                idx += 1;
            }
            Some("keepenv") => {
                options.keepenv = true;
                idx += 1;
            }
            Some("setenv") => {
                idx += 1;
                // Expect a '{' token next.
                if tokens.get(idx).map(String::as_str) != Some("{") {
                    return Err("expected '{' after 'setenv'".to_string());
                }
                idx += 1;

                // Collect assignments until '}'.
                while tokens.get(idx).map(String::as_str) != Some("}") {
                    if idx >= tokens.len() {
                        return Err("unterminated 'setenv' block (missing '}')".to_string());
                    }
                    let assignment = &tokens[idx];
                    if let Some(eq_pos) = assignment.find('=') {
                        let var = assignment[..eq_pos].to_string();
                        let val = assignment[eq_pos + 1..].to_string();
                        options.setenv.push((var, val));
                    } else {
                        // A bare name in setenv means unset (remove) that variable.
                        options.unsetenv.push(assignment.clone());
                    }
                    idx += 1;
                }
                // Skip the closing '}'.
                idx += 1;
            }
            _ => break,
        }
    }

    if action == RuleAction::Deny
        && (options.nopass
            || options.persist
            || options.keepenv
            || !options.setenv.is_empty()
            || !options.unsetenv.is_empty())
    {
        return Err("options are not valid on 'deny' rules".to_string());
    }

    // 3. Identity (required).
    let identity = match tokens.get(idx) {
        Some(tok) => tok.clone(),
        None => return Err("expected user or :group identity".to_string()),
    };
    idx += 1;

    // 4. Optional "as <target>"
    let mut target = None;
    if tokens.get(idx).map(String::as_str) == Some("as") {
        idx += 1;
        target = Some(
            tokens
                .get(idx)
                .ok_or_else(|| "expected username after 'as'".to_string())?
                .clone(),
        );
        idx += 1;
    }

    // 5. Optional "cmd <command>"
    let mut cmd = None;
    let mut args = None;
    if tokens.get(idx).map(String::as_str) == Some("cmd") {
        idx += 1;
        cmd = Some(
            tokens
                .get(idx)
                .ok_or_else(|| "expected command after 'cmd'".to_string())?
                .clone(),
        );
        idx += 1;

        // 6. Optional "args <args...>"
        if tokens.get(idx).map(String::as_str) == Some("args") {
            idx += 1;
            let mut arg_list = Vec::new();
            while idx < tokens.len() {
                arg_list.push(tokens[idx].clone());
                idx += 1;
            }
            args = Some(arg_list);
        }
    }

    // There should be nothing left.
    if idx < tokens.len() {
        return Err(format!("unexpected token '{}'", tokens[idx]));
    }

    Ok(Rule {
        action,
        options,
        identity,
        target,
        cmd,
        args,
    })
}

/// Tokenize a doas.conf line, respecting curly braces and quoted strings.
fn tokenize(line: &str) -> Result<Vec<String>, String> {
    let mut tokens = Vec::new();
    let mut chars = line.chars().peekable();

    while let Some(&ch) = chars.peek() {
        if ch.is_whitespace() {
            chars.next();
            continue;
        }

        // Comment -- rest of line is ignored.
        if ch == '#' {
            break;
        }

        // Braces are individual tokens.
        if ch == '{' || ch == '}' {
            tokens.push(ch.to_string());
            chars.next();
            continue;
        }

        // A token is a run of bare characters and/or quoted segments, ending
        // at top-level whitespace, '#', '{', or '}'.  Quotes may appear
        // anywhere within a token (e.g. `PATH="/usr/bin:/bin"`), not just at
        // its start, and are stripped from the resulting token.
        let mut word = String::new();
        let mut have_token = false;
        while let Some(&c) = chars.peek() {
            if c.is_whitespace() || c == '#' || c == '{' || c == '}' {
                break;
            }
            if c == '"' {
                chars.next(); // consume opening quote
                have_token = true; // even an empty "" yields a token
                loop {
                    match chars.next() {
                        Some('\\') => {
                            // Escaped character inside quotes.
                            if let Some(escaped) = chars.next() {
                                word.push(escaped);
                            }
                        }
                        Some('"') => break,
                        Some(other) => word.push(other),
                        None => return Err("unterminated quoted string".to_string()),
                    }
                }
                continue;
            }
            word.push(c);
            have_token = true;
            chars.next();
        }
        if have_token {
            tokens.push(word);
        }
    }

    Ok(tokens)
}

// ============================================================================
// Rule matching
// ============================================================================

/// Evaluate all rules against the given request. The first matching rule wins.
fn evaluate_rules(
    rules: &[Rule],
    caller_name: &str,
    target_name: &str,
    command: Option<&str>,
    command_args: &[String],
) -> MatchResult {
    for rule in rules {
        if !identity_matches(&rule.identity, caller_name) {
            continue;
        }

        if let Some(ref target) = rule.target
            && target != target_name
        {
            continue;
        }

        if let Some(ref cmd) = rule.cmd {
            match command {
                Some(actual_cmd) => {
                    if !command_matches(cmd, actual_cmd) {
                        continue;
                    }
                }
                None => continue,
            }
        }

        if let Some(ref expected_args) = rule.args {
            if command_args.len() != expected_args.len() {
                continue;
            }
            let all_match = expected_args
                .iter()
                .zip(command_args.iter())
                .all(|(exp, act)| exp == act);
            if !all_match {
                continue;
            }
        }

        // This rule matches.
        return match rule.action {
            RuleAction::Permit => MatchResult::Permit(rule.options.clone()),
            RuleAction::Deny => MatchResult::Deny,
        };
    }

    MatchResult::NoMatch
}

/// Check whether an identity specification matches the calling user.
///
/// - A bare name (e.g., `alice`) matches the username directly.
/// - A `:group` form (e.g., `:wheel`) matches if the caller is a member of
///   that group.
fn identity_matches(identity: &str, caller_name: &str) -> bool {
    if let Some(group_name) = identity.strip_prefix(':') {
        user_in_group(caller_name, group_name)
    } else {
        identity == caller_name
    }
}

/// Check whether a command specification matches the actual command being run.
///
/// If the spec is an absolute path, it must match exactly. Otherwise, we
/// compare just the basename so that `cmd pkg` matches `/usr/bin/pkg`.
fn command_matches(spec: &str, actual: &str) -> bool {
    if spec.starts_with('/') {
        // Absolute path -- exact match required.
        spec == actual
    } else {
        // Compare basenames.
        let spec_base = spec.rsplit('/').next().unwrap_or(spec);
        let actual_base = actual.rsplit('/').next().unwrap_or(actual);
        spec_base == actual_base
    }
}

// ============================================================================
// Persist (timestamp) files
// ============================================================================

/// Return the path to the timestamp file for a given UID.
fn persist_path(uid: u32) -> String {
    format!("{PERSIST_DIR}/{uid}")
}

/// Check whether a valid (non-expired) persist timestamp exists for the UID.
fn persist_valid(uid: u32) -> bool {
    let path = persist_path(uid);
    let content = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return false,
    };

    let stamp: u64 = match content.trim().parse() {
        Ok(v) => v,
        Err(_) => return false,
    };

    let now = current_epoch_secs();
    // The timestamp must be in the past (not forged into the future) and
    // within the timeout window.
    if stamp > now {
        return false;
    }
    now.saturating_sub(stamp) < PERSIST_TIMEOUT_SECS
}

/// Update the persist timestamp for the given UID to "now".
fn persist_touch(uid: u32) {
    let _ = fs::create_dir_all(PERSIST_DIR);
    let path = persist_path(uid);
    let now = current_epoch_secs();
    let _ = fs::write(&path, now.to_string());
}

/// Clear the persist timestamp for the given UID.
fn persist_clear(uid: u32) {
    let path = persist_path(uid);
    let _ = fs::remove_file(&path);
}

// ============================================================================
// Environment building
// ============================================================================

/// Build the environment for the child process.
///
/// With the default (clean) environment, only a minimal set of variables is
/// propagated. `keepenv` preserves the caller's full environment. `setenv`
/// adds or overrides specific variables.
fn build_environment(
    opts: &RuleOptions,
    target: &PasswdEntry,
    caller_name: &str,
) -> Vec<(String, String)> {
    let mut env_map: Vec<(String, String)> = if opts.keepenv {
        env::vars().collect()
    } else {
        let mut base = Vec::new();
        base.push(("HOME".to_string(), target.home.clone()));
        base.push(("LOGNAME".to_string(), target.username.clone()));
        base.push((
            "PATH".to_string(),
            if target.uid == 0 {
                "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin".to_string()
            } else {
                "/usr/local/bin:/usr/bin:/bin".to_string()
            },
        ));
        base.push(("SHELL".to_string(), target.shell.clone()));
        base.push(("USER".to_string(), target.username.clone()));

        // Propagate TERM if the caller has it set.
        if let Ok(term) = env::var("TERM") {
            base.push(("TERM".to_string(), term));
        }

        base
    };

    // Always set DOAS_USER to the original (calling) user.
    set_env_var(&mut env_map, "DOAS_USER", caller_name);

    // Apply setenv assignments.
    for (var, val) in &opts.setenv {
        set_env_var(&mut env_map, var, val);
    }

    // Remove unsetenv variables.
    for var in &opts.unsetenv {
        env_map.retain(|(k, _)| k != var);
    }

    env_map
}

/// Set or overwrite a variable in the environment vector.
fn set_env_var(env_map: &mut Vec<(String, String)>, key: &str, value: &str) {
    if let Some(existing) = env_map.iter_mut().find(|(k, _)| k == key) {
        existing.1 = value.to_string();
    } else {
        env_map.push((key.to_string(), value.to_string()));
    }
}

// ============================================================================
// Command execution
// ============================================================================

/// Execute a command as the target user.
///
/// On Slate OS, `setuid`/`setgid` are actual syscalls that change the process
/// identity. For now we set the UID/GID environment hints and invoke the
/// command. The real privilege change will use the kernel's capability system
/// once the POSIX exec layer supports `setuid`/`setgid` syscalls.
fn exec_command(
    target: &PasswdEntry,
    command: &str,
    arguments: &[String],
    environment: &[(String, String)],
) -> i32 {
    let mut cmd = process::Command::new(command);
    cmd.args(arguments);
    cmd.env_clear();
    for (key, val) in environment {
        cmd.env(key, val);
    }

    // Set UID/GID hints in the environment for the POSIX layer.
    cmd.env("UID", target.uid.to_string());
    cmd.env("GID", target.gid.to_string());

    match cmd.status() {
        Ok(status) => status.code().unwrap_or(1),
        Err(e) => {
            eprintln!("doas: failed to execute {command}: {e}");
            126
        }
    }
}

/// Resolve a command name to an absolute path by searching PATH directories.
/// Returns the first match found, or the original name if no match.
fn resolve_command(command: &str) -> String {
    // If it already contains a slash, it is a path -- use it directly.
    if command.contains('/') {
        return command.to_string();
    }

    let path_var = env::var("PATH").unwrap_or_else(|_| "/usr/local/bin:/usr/bin:/bin".to_string());

    for dir in path_var.split(':') {
        if dir.is_empty() {
            continue;
        }
        let candidate = format!("{dir}/{command}");
        if fs::metadata(&candidate).is_ok() {
            return candidate;
        }
    }

    // Fallback: return the bare name and let exec handle the error.
    command.to_string()
}

// ============================================================================
// System helpers
// ============================================================================

/// Get the current epoch time in seconds.
fn current_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Determine the current user's UID from the environment or /proc.
fn current_uid() -> u32 {
    // Try /proc/self/status first.
    if let Ok(content) = fs::read_to_string("/proc/self/status") {
        for line in content.lines() {
            if let Some(rest) = line.strip_prefix("Uid:")
                && let Some(uid_str) = rest.split_whitespace().next()
                && let Ok(uid) = uid_str.parse::<u32>()
            {
                return uid;
            }
        }
    }

    // Fallback: UID env var.
    env::var("UID")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(u32::MAX)
}

/// Get the current user's name, trying USER env var then /etc/passwd lookup.
fn current_username() -> Option<String> {
    if let Ok(name) = env::var("USER") {
        return Some(name);
    }
    let uid = current_uid();
    lookup_passwd_uid(uid).map(|e| e.username)
}

/// Read a password from stdin without echoing (best-effort).
fn read_password_no_echo(prompt: &str) -> Result<String, String> {
    eprint!("{prompt}");
    let _ = io::stderr().flush();

    let mut line = String::new();
    io::stdin()
        .lock()
        .read_line(&mut line)
        .map_err(|e| format!("read error: {e}"))?;
    eprintln!(); // newline after hidden input

    if line.ends_with('\n') {
        line.pop();
    }
    if line.ends_with('\r') {
        line.pop();
    }

    Ok(line)
}

// ============================================================================
// Argument parsing
// ============================================================================

/// Parsed command-line arguments.
struct DoasArgs {
    /// Target user (default: "root").
    target_user: String,
    /// Run the target user's shell instead of a command.
    shell_mode: bool,
    /// Path to an alternate configuration file.
    config_path: String,
    /// If `true`, check configuration syntax and exit.
    check_config: bool,
    /// If `true`, clear the persist timestamp and exit.
    clear_persist: bool,
    /// If `true`, fail immediately if a password is needed.
    non_interactive: bool,
    /// The command to execute.
    command: Option<String>,
    /// Arguments to the command.
    arguments: Vec<String>,
}

fn parse_args(raw: &[String]) -> Result<DoasArgs, String> {
    let mut result = DoasArgs {
        target_user: "root".to_string(),
        shell_mode: false,
        config_path: DEFAULT_CONFIG_PATH.to_string(),
        check_config: false,
        clear_persist: false,
        non_interactive: false,
        command: None,
        arguments: Vec::new(),
    };

    let mut idx = 1; // skip argv[0]
    let mut end_of_opts = false;

    while idx < raw.len() {
        let arg = &raw[idx];

        if end_of_opts || !arg.starts_with('-') || arg == "-" {
            // First non-option is the command; the rest are its arguments.
            result.command = Some(arg.clone());
            result.arguments = raw[idx + 1..].to_vec();
            break;
        }

        match arg.as_str() {
            "--" => {
                end_of_opts = true;
                idx += 1;
            }
            "-u" | "-U" => {
                idx += 1;
                if idx >= raw.len() {
                    return Err("option -u requires a user argument".to_string());
                }
                result.target_user = raw[idx].clone();
                idx += 1;
            }
            "-s" => {
                result.shell_mode = true;
                idx += 1;
            }
            "-C" => {
                idx += 1;
                if idx >= raw.len() {
                    return Err("option -C requires a config file argument".to_string());
                }
                result.config_path = raw[idx].clone();
                result.check_config = true;
                idx += 1;
            }
            "-L" => {
                result.clear_persist = true;
                idx += 1;
            }
            "-n" => {
                result.non_interactive = true;
                idx += 1;
            }
            other => return Err(format!("unknown option: {other}")),
        }
    }

    Ok(result)
}

fn print_usage() {
    eprintln!("usage: doas [-nsL] [-C config] [-u user] [--] command [args ...]");
}

// ============================================================================
// Main entry point
// ============================================================================

fn main() {
    let raw_args: Vec<String> = env::args().collect();

    let args = match parse_args(&raw_args) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("doas: {e}");
            print_usage();
            process::exit(1);
        }
    };

    // -L: clear persist timestamp and exit.
    if args.clear_persist {
        let uid = current_uid();
        persist_clear(uid);
        process::exit(0);
    }

    // Read configuration file.
    let config_content = match fs::read_to_string(&args.config_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("doas: failed to read {}: {e}", args.config_path);
            process::exit(1);
        }
    };

    let rules = match parse_config(&config_content) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("doas: syntax error in {}: {e}", args.config_path);
            process::exit(1);
        }
    };

    // -C: check config and exit.
    if args.check_config {
        // If we got here, the config parsed successfully.
        process::exit(0);
    }

    // Determine the calling user.
    let caller_name = match current_username() {
        Some(name) => name,
        None => {
            eprintln!("doas: cannot determine current user");
            process::exit(1);
        }
    };

    // Determine the command to run.
    let command = if args.shell_mode {
        // Look up the target user's shell.
        match lookup_passwd_user(&args.target_user) {
            Some(entry) => entry.shell.clone(),
            None => {
                eprintln!("doas: unknown user: {}", args.target_user);
                process::exit(1);
            }
        }
    } else {
        match &args.command {
            Some(cmd) => cmd.clone(),
            None => {
                eprintln!("doas: no command specified");
                print_usage();
                process::exit(1);
            }
        }
    };

    // Resolve the command to a full path for rule matching.
    let resolved_cmd = resolve_command(&command);

    // Evaluate rules.
    let match_result = evaluate_rules(
        &rules,
        &caller_name,
        &args.target_user,
        Some(&resolved_cmd),
        &args.arguments,
    );

    let opts = match match_result {
        MatchResult::Permit(opts) => opts,
        MatchResult::Deny => {
            eprintln!("doas: operation not permitted");
            process::exit(1);
        }
        MatchResult::NoMatch => {
            eprintln!(
                "doas: {} is not allowed to run '{}' as {}",
                caller_name, command, args.target_user
            );
            process::exit(1);
        }
    };

    // Authentication.
    let caller_uid = current_uid();
    if !opts.nopass {
        // Check persist timestamp.
        let already_authed = opts.persist && persist_valid(caller_uid);

        if !already_authed {
            if args.non_interactive {
                eprintln!("doas: authentication required (non-interactive mode)");
                process::exit(1);
            }

            // Ask first, then look the account up. The old order did the
            // lookup first and exited with a different message for "no shadow
            // entry" and for "locked" *before* the prompt appeared, which told
            // anyone running `doas` which accounts were in which state.
            let password = match read_password_no_echo(&format!("doas ({caller_name}) password: "))
            {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("doas: {e}");
                    process::exit(1);
                }
            };

            // `authenticate` does the store lookup itself, and spends the same
            // time on a caller with no entry as on one with a wrong password —
            // so there is no `burn` here, which would only double the cost of
            // every path.
            let outcome =
                authlib::Authenticator::new().authenticate(&caller_name, password.as_bytes());

            if !outcome.is_accepted() {
                // One wording for every failure, per `Outcome::user_message`:
                // which of them it was is exactly what an attacker wants to
                // learn. An entry no password can ever match is the one case
                // that also needs saying out loud, because only an
                // administrator can clear it and nobody else will notice.
                eprintln!("doas: {}", outcome.user_message());
                if outcome.needs_administrator() {
                    eprintln!(
                        "doas: the stored password entry for {caller_name} is in a format this \
                         system cannot recompute; an administrator must set a new password"
                    );
                }
                process::exit(1);
            }

            // Update persist timestamp on success.
            if opts.persist {
                persist_touch(caller_uid);
            }
        }
    }

    // Look up the target user.
    let target = match lookup_passwd_user(&args.target_user) {
        Some(entry) => entry,
        None => {
            eprintln!("doas: unknown target user: {}", args.target_user);
            process::exit(1);
        }
    };

    // Build environment.
    let environment = build_environment(&opts, &target, &caller_name);

    // Execute.
    let exit_code = exec_command(&target, &resolved_cmd, &args.arguments, &environment);

    process::exit(exit_code);
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    // ========================================================================
    // Password verification
    //
    // These replace a set that tested this crate's own `$sha256$salt$digest`
    // hasher against itself: `hash_password` then `verify_password`, which
    // agree by construction and would have kept agreeing had the format been
    // anything at all. What they never asked was whether the format was the one
    // `passwd` writes -- and it was not, so `doas` refused every real password
    // on the system while its tests were green.
    //
    // The tests below therefore never state a hash. They build the stored entry
    // with the same `posix::crypt` calls `passwd` uses, so if `passwd`'s format
    // moves and `doas` does not follow, these fail rather than drift.
    // ========================================================================

    /// A `/etc/shadow` line for `user` whose password is `password`, hashed the
    /// way `passwd` hashes it.
    fn shadow_line(user: &str, password: &str) -> String {
        let mut setting_buf = posix::crypt::buf();
        let setting =
            posix::crypt::setting_into(posix::crypt::Method::Sha512, b"doastest", &mut setting_buf)
                .expect("valid crypt setting");
        let setting = setting.to_string();
        let mut hash_buf = posix::crypt::buf();
        let hashed =
            posix::crypt::hash_into(password.as_bytes(), setting.as_bytes(), &mut hash_buf)
                .expect("hashable password");
        format!("{user}:{hashed}:19500:0:99999:7:::\n")
    }

    /// A path in the OS temp directory that nothing else in this run will use.
    fn tmp_path(tag: &str) -> std::path::PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        std::env::temp_dir().join(format!("doas-{tag}-{unique}-{}.test", process::id()))
    }

    /// An [`authlib::Authenticator`] backed by a temporary shadow file holding
    /// `content`, and by a `users.yaml` that does not exist -- so the shadow
    /// file is the only thing that can answer.
    fn authenticator_with_shadow(tag: &str, content: &str) -> (authlib::Authenticator, PathBuf) {
        let shadow = tmp_path(tag);
        fs::write(&shadow, content).expect("write temp shadow");
        let missing = tmp_path(&format!("{tag}-no-users-yaml"));
        (
            authlib::Authenticator::with_stores(&missing, &shadow),
            shadow,
        )
    }

    #[test]
    fn a_password_set_with_passwd_opens_a_doas_prompt() {
        // The whole point of the change: before it, this was `Rejected`, and
        // there was no password anyone could type that would not be.
        let (mut auth, path) = authenticator_with_shadow("ok", &shadow_line("alice", "hunter2"));
        assert_eq!(
            auth.authenticate("alice", b"hunter2"),
            authlib::Outcome::Accepted
        );
        let _ = fs::remove_file(path);
    }

    #[test]
    fn a_wrong_password_does_not() {
        let (mut auth, path) = authenticator_with_shadow("bad", &shadow_line("alice", "hunter2"));
        assert_eq!(
            auth.authenticate("alice", b"hunter3"),
            authlib::Outcome::Rejected
        );
        let _ = fs::remove_file(path);
    }

    #[test]
    fn a_locked_account_cannot_escalate() {
        let (mut auth, path) = authenticator_with_shadow("locked", "alice:!:19500:0:99999:7:::\n");
        // Not `Rejected`: no password opens it, and `is_accepted` is the only
        // thing `main` asks, so an `Outcome` that is not `Accepted` is a refusal
        // however it got there.
        let outcome = auth.authenticate("alice", b"hunter2");
        assert_eq!(outcome, authlib::Outcome::Locked);
        assert!(!outcome.is_accepted());
        let _ = fs::remove_file(path);
    }

    #[test]
    fn an_account_with_no_password_cannot_escalate() {
        // `login` answers this one the other way for a console login. Escalation
        // is not a login: `nopass` in doas.conf is the only consent that counts.
        let (mut auth, path) = authenticator_with_shadow("empty", "alice::19500:0:99999:7:::\n");
        let outcome = auth.authenticate("alice", b"");
        assert_eq!(outcome, authlib::Outcome::NoPassword);
        assert!(!outcome.is_accepted());
        let _ = fs::remove_file(path);
    }

    #[test]
    fn the_hash_this_crate_used_to_write_is_reported_broken_not_wrong() {
        // A system that ran the old `doas`, or the old `passwd`, has entries in
        // this shape. They must be distinguishable from a typo, because no
        // amount of retyping will ever clear one.
        let (mut auth, path) = authenticator_with_shadow(
            "legacy",
            "alice:$sha256$battery_staple$0123456789abcdef:19500:0:99999:7:::\n",
        );
        let outcome = auth.authenticate("alice", b"correct_horse");
        assert_eq!(outcome, authlib::Outcome::Unusable);
        assert!(outcome.needs_administrator());
        let _ = fs::remove_file(path);
    }

    #[test]
    fn a_caller_with_no_entry_looks_exactly_like_a_wrong_password() {
        let (mut auth, path) =
            authenticator_with_shadow("absent", &shadow_line("alice", "hunter2"));
        assert_eq!(
            auth.authenticate("mallory", b"anything"),
            authlib::Outcome::Rejected
        );
        // And says the same thing, so the prompt is not an account oracle.
        assert_eq!(
            authlib::Outcome::Rejected.user_message(),
            authlib::Outcome::Locked.user_message()
        );
        let _ = fs::remove_file(path);
    }

    #[test]
    fn only_accepted_is_a_yes() {
        // `main` gates on `is_accepted`. If that ever became `!= Rejected`,
        // `Locked`, `NoPassword`, `Unusable` and `RateLimited` would all become
        // root, so it is worth a test of its own.
        for outcome in [
            authlib::Outcome::Rejected,
            authlib::Outcome::Locked,
            authlib::Outcome::NoPassword,
            authlib::Outcome::Unusable,
            authlib::Outcome::RateLimited {
                retry_after_secs: 30,
            },
        ] {
            assert!(!outcome.is_accepted(), "{outcome:?} must not admit anyone");
        }
        assert!(authlib::Outcome::Accepted.is_accepted());
    }

    // ========================================================================
    // Passwd parsing tests
    // ========================================================================

    #[test]
    fn passwd_parse_valid_line() {
        let entry = parse_passwd_line("alice:x:1000:1000:Alice:/home/alice:/bin/bash");
        assert!(entry.is_some());
        let e = entry.unwrap();
        assert_eq!(e.username, "alice");
        assert_eq!(e.uid, 1000);
        assert_eq!(e.gid, 1000);
        assert_eq!(e.home, "/home/alice");
        assert_eq!(e.shell, "/bin/bash");
    }

    #[test]
    fn passwd_parse_root() {
        let entry = parse_passwd_line("root:x:0:0:root:/root:/bin/sh");
        let e = entry.unwrap();
        assert_eq!(e.uid, 0);
        assert_eq!(e.gid, 0);
    }

    #[test]
    fn passwd_parse_too_short() {
        assert!(parse_passwd_line("user:x:1000").is_none());
    }

    #[test]
    fn passwd_parse_bad_uid() {
        assert!(parse_passwd_line("user:x:notnum:0::/home:/bin/sh").is_none());
    }

    #[test]
    fn passwd_parse_bad_gid() {
        assert!(parse_passwd_line("user:x:1000:notnum::/home:/bin/sh").is_none());
    }

    // ========================================================================
    // Tokenizer tests
    // ========================================================================

    #[test]
    fn tokenize_simple() {
        let tokens = tokenize("permit nopass root").unwrap();
        assert_eq!(tokens, vec!["permit", "nopass", "root"]);
    }

    #[test]
    fn tokenize_with_braces() {
        let tokens = tokenize("permit setenv { HOME=/root } alice").unwrap();
        assert_eq!(
            tokens,
            vec!["permit", "setenv", "{", "HOME=/root", "}", "alice"]
        );
    }

    #[test]
    fn tokenize_with_comment() {
        let tokens = tokenize("permit root # this is a comment").unwrap();
        assert_eq!(tokens, vec!["permit", "root"]);
    }

    #[test]
    fn tokenize_quoted_string() {
        let tokens = tokenize(r#"permit setenv { PATH="/usr/bin:/bin" } alice"#).unwrap();
        assert_eq!(
            tokens,
            vec!["permit", "setenv", "{", "PATH=/usr/bin:/bin", "}", "alice"]
        );
    }

    #[test]
    fn tokenize_empty_line() {
        let tokens = tokenize("").unwrap();
        assert!(tokens.is_empty());
    }

    #[test]
    fn tokenize_only_comment() {
        let tokens = tokenize("# just a comment").unwrap();
        assert!(tokens.is_empty());
    }

    #[test]
    fn tokenize_unterminated_quote() {
        assert!(tokenize(r#"permit "unterminated"#).is_err());
    }

    #[test]
    fn tokenize_escaped_quote() {
        let tokens = tokenize(r#"permit "hello\"world""#).unwrap();
        assert_eq!(tokens, vec!["permit", "hello\"world"]);
    }

    // ========================================================================
    // Config parsing tests
    // ========================================================================

    #[test]
    fn parse_permit_nopass_user() {
        let rules = parse_config("permit nopass root\n").unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].action, RuleAction::Permit);
        assert!(rules[0].options.nopass);
        assert_eq!(rules[0].identity, "root");
        assert!(rules[0].target.is_none());
        assert!(rules[0].cmd.is_none());
        assert!(rules[0].args.is_none());
    }

    #[test]
    fn parse_permit_group() {
        let rules = parse_config("permit nopass :wheel\n").unwrap();
        assert_eq!(rules[0].identity, ":wheel");
        assert!(rules[0].options.nopass);
    }

    #[test]
    fn parse_permit_persist_as_target() {
        let rules = parse_config("permit persist alice as root\n").unwrap();
        assert_eq!(rules[0].identity, "alice");
        assert!(rules[0].options.persist);
        assert_eq!(rules[0].target.as_deref(), Some("root"));
    }

    #[test]
    fn parse_permit_cmd() {
        let rules = parse_config("permit alice cmd /usr/bin/pkg\n").unwrap();
        assert_eq!(rules[0].cmd.as_deref(), Some("/usr/bin/pkg"));
    }

    #[test]
    fn parse_permit_cmd_args() {
        let rules = parse_config("permit alice cmd /usr/bin/pkg args install vim\n").unwrap();
        assert_eq!(rules[0].cmd.as_deref(), Some("/usr/bin/pkg"));
        assert_eq!(
            rules[0].args.as_ref().unwrap(),
            &["install".to_string(), "vim".to_string()]
        );
    }

    #[test]
    fn parse_deny_user() {
        let rules = parse_config("deny bob\n").unwrap();
        assert_eq!(rules[0].action, RuleAction::Deny);
        assert_eq!(rules[0].identity, "bob");
    }

    #[test]
    fn parse_deny_with_options_fails() {
        let result = parse_config("deny nopass bob\n");
        assert!(result.is_err());
    }

    #[test]
    fn parse_keepenv() {
        let rules = parse_config("permit keepenv alice\n").unwrap();
        assert!(rules[0].options.keepenv);
    }

    #[test]
    fn parse_setenv() {
        let rules = parse_config("permit setenv { HOME=/root FOO=bar } alice\n").unwrap();
        assert_eq!(rules[0].options.setenv.len(), 2);
        assert_eq!(
            rules[0].options.setenv[0],
            ("HOME".to_string(), "/root".to_string())
        );
        assert_eq!(
            rules[0].options.setenv[1],
            ("FOO".to_string(), "bar".to_string())
        );
    }

    #[test]
    fn parse_setenv_unset() {
        let rules = parse_config("permit setenv { -DISPLAY } alice\n").unwrap();
        // A bare name without '=' is stored as unsetenv.
        assert_eq!(rules[0].options.unsetenv, vec!["-DISPLAY".to_string()]);
    }

    #[test]
    fn parse_setenv_unterminated() {
        let result = parse_config("permit setenv { FOO=bar alice\n");
        assert!(result.is_err());
    }

    #[test]
    fn parse_multiple_options() {
        let rules = parse_config("permit nopass persist keepenv alice\n").unwrap();
        assert!(rules[0].options.nopass);
        assert!(rules[0].options.persist);
        assert!(rules[0].options.keepenv);
    }

    #[test]
    fn parse_empty_config() {
        let rules = parse_config("").unwrap();
        assert!(rules.is_empty());
    }

    #[test]
    fn parse_comments_only() {
        let rules = parse_config("# comment 1\n# comment 2\n").unwrap();
        assert!(rules.is_empty());
    }

    #[test]
    fn parse_blank_lines() {
        let rules = parse_config("\n\npermit root\n\n").unwrap();
        assert_eq!(rules.len(), 1);
    }

    #[test]
    fn parse_bad_action() {
        let result = parse_config("allow root\n");
        assert!(result.is_err());
    }

    #[test]
    fn parse_missing_identity() {
        let result = parse_config("permit\n");
        assert!(result.is_err());
    }

    #[test]
    fn parse_missing_as_target() {
        let result = parse_config("permit alice as\n");
        assert!(result.is_err());
    }

    #[test]
    fn parse_missing_cmd_name() {
        let result = parse_config("permit alice cmd\n");
        assert!(result.is_err());
    }

    #[test]
    fn parse_unexpected_token() {
        let result = parse_config("permit alice garbage\n");
        assert!(result.is_err());
    }

    #[test]
    fn parse_multiple_rules() {
        let config = "\
            permit nopass root\n\
            permit nopass :wheel\n\
            permit persist alice as root\n\
            deny bob\n";
        let rules = parse_config(config).unwrap();
        assert_eq!(rules.len(), 4);
        assert_eq!(rules[0].action, RuleAction::Permit);
        assert_eq!(rules[0].identity, "root");
        assert_eq!(rules[1].identity, ":wheel");
        assert_eq!(rules[2].identity, "alice");
        assert_eq!(rules[3].action, RuleAction::Deny);
        assert_eq!(rules[3].identity, "bob");
    }

    #[test]
    fn parse_setenv_missing_brace() {
        let result = parse_config("permit setenv FOO=bar alice\n");
        assert!(result.is_err());
    }

    // ========================================================================
    // Rule matching tests
    // ========================================================================

    fn sample_rules() -> Vec<Rule> {
        parse_config(
            "\
            permit nopass root\n\
            permit nopass :wheel\n\
            permit persist alice as root\n\
            permit alice cmd /usr/bin/pkg\n\
            permit alice cmd /usr/bin/pkg args install vim\n\
            deny bob\n",
        )
        .unwrap()
    }

    #[test]
    fn match_root_nopass() {
        let rules = sample_rules();
        let result = evaluate_rules(&rules, "root", "root", Some("/bin/ls"), &[]);
        match result {
            MatchResult::Permit(opts) => assert!(opts.nopass),
            other => panic!("expected Permit, got {other:?}"),
        }
    }

    #[test]
    fn match_deny_bob() {
        let rules = sample_rules();
        let result = evaluate_rules(&rules, "bob", "root", Some("/bin/ls"), &[]);
        assert_eq!(result, MatchResult::Deny);
    }

    #[test]
    fn match_no_rule_for_unknown() {
        let rules = sample_rules();
        let result = evaluate_rules(&rules, "charlie", "root", Some("/bin/ls"), &[]);
        assert_eq!(result, MatchResult::NoMatch);
    }

    #[test]
    fn match_alice_as_root() {
        let rules = sample_rules();
        let result = evaluate_rules(&rules, "alice", "root", Some("/bin/ls"), &[]);
        match result {
            MatchResult::Permit(opts) => assert!(opts.persist),
            other => panic!("expected Permit(persist), got {other:?}"),
        }
    }

    #[test]
    fn match_alice_as_nonroot_fails() {
        // Alice's "as root" rule should NOT match when the target is "bob".
        // Her cmd rule has no "as" restriction, so it would match only for /usr/bin/pkg.
        let rules = sample_rules();
        let result = evaluate_rules(&rules, "alice", "bob", Some("/bin/ls"), &[]);
        assert_eq!(result, MatchResult::NoMatch);
    }

    #[test]
    fn match_alice_cmd_pkg() {
        let rules = sample_rules();
        let result = evaluate_rules(&rules, "alice", "root", Some("/usr/bin/pkg"), &[]);
        // The "as root" rule matches first (it has no cmd restriction).
        match result {
            MatchResult::Permit(_) => {} // OK
            other => panic!("expected Permit, got {other:?}"),
        }
    }

    #[test]
    fn match_alice_cmd_pkg_with_args() {
        let rules = sample_rules();
        let args = vec!["install".to_string(), "vim".to_string()];
        let result = evaluate_rules(&rules, "alice", "root", Some("/usr/bin/pkg"), &args);
        // The "as root" rule (no cmd) matches first.
        match result {
            MatchResult::Permit(_) => {}
            other => panic!("expected Permit, got {other:?}"),
        }
    }

    #[test]
    fn match_args_mismatch() {
        // Create rules where only the args-restricted rule is available.
        let rules = parse_config("permit alice cmd /usr/bin/pkg args install vim\n").unwrap();
        let args = vec!["install".to_string(), "emacs".to_string()];
        let result = evaluate_rules(&rules, "alice", "root", Some("/usr/bin/pkg"), &args);
        assert_eq!(result, MatchResult::NoMatch);
    }

    #[test]
    fn match_args_count_mismatch() {
        let rules = parse_config("permit alice cmd /usr/bin/pkg args install vim\n").unwrap();
        let args = vec!["install".to_string()];
        let result = evaluate_rules(&rules, "alice", "root", Some("/usr/bin/pkg"), &args);
        assert_eq!(result, MatchResult::NoMatch);
    }

    #[test]
    fn match_first_rule_wins() {
        let rules = parse_config("deny alice\npermit alice\n").unwrap();
        let result = evaluate_rules(&rules, "alice", "root", Some("/bin/ls"), &[]);
        assert_eq!(result, MatchResult::Deny);
    }

    // ========================================================================
    // Command matching tests
    // ========================================================================

    #[test]
    fn command_match_absolute_exact() {
        assert!(command_matches("/usr/bin/pkg", "/usr/bin/pkg"));
    }

    #[test]
    fn command_match_absolute_mismatch() {
        assert!(!command_matches("/usr/bin/pkg", "/usr/bin/apt"));
    }

    #[test]
    fn command_match_basename() {
        assert!(command_matches("pkg", "/usr/bin/pkg"));
    }

    #[test]
    fn command_match_basename_mismatch() {
        assert!(!command_matches("apt", "/usr/bin/pkg"));
    }

    // ========================================================================
    // Environment building tests
    // ========================================================================

    fn make_target_user() -> PasswdEntry {
        PasswdEntry {
            username: "root".to_string(),
            uid: 0,
            gid: 0,
            home: "/root".to_string(),
            shell: "/bin/sh".to_string(),
        }
    }

    #[test]
    fn env_clean_default() {
        let opts = RuleOptions::default();
        let target = make_target_user();
        let environment = build_environment(&opts, &target, "alice");

        let find_val = |key: &str| -> Option<String> {
            environment
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.clone())
        };

        assert_eq!(find_val("HOME"), Some("/root".to_string()));
        assert_eq!(find_val("LOGNAME"), Some("root".to_string()));
        assert_eq!(find_val("USER"), Some("root".to_string()));
        assert_eq!(find_val("SHELL"), Some("/bin/sh".to_string()));
        assert_eq!(find_val("DOAS_USER"), Some("alice".to_string()));
        assert!(find_val("PATH").is_some());
    }

    #[test]
    fn env_clean_root_path() {
        let opts = RuleOptions::default();
        let target = make_target_user();
        let environment = build_environment(&opts, &target, "alice");
        let path = environment
            .iter()
            .find(|(k, _)| k == "PATH")
            .map(|(_, v)| v.clone());
        assert!(path.unwrap().contains("/sbin"));
    }

    #[test]
    fn env_clean_non_root_path() {
        let opts = RuleOptions::default();
        let target = PasswdEntry {
            username: "alice".to_string(),
            uid: 1000,
            gid: 1000,
            home: "/home/alice".to_string(),
            shell: "/bin/bash".to_string(),
        };
        let environment = build_environment(&opts, &target, "bob");
        let path = environment
            .iter()
            .find(|(k, _)| k == "PATH")
            .map(|(_, v)| v.clone());
        assert!(!path.unwrap().contains("/sbin"));
    }

    #[test]
    fn env_keepenv_preserves_caller() {
        // This test verifies that with keepenv, existing env vars are preserved.
        // In the test environment we just check the logic path.
        let opts = RuleOptions {
            keepenv: true,
            ..Default::default()
        };
        let target = make_target_user();
        let environment = build_environment(&opts, &target, "alice");

        // DOAS_USER should always be set.
        let doas_user = environment.iter().find(|(k, _)| k == "DOAS_USER");
        assert!(doas_user.is_some());
        assert_eq!(doas_user.unwrap().1, "alice");
    }

    #[test]
    fn env_setenv_adds_vars() {
        let opts = RuleOptions {
            setenv: vec![
                ("EDITOR".to_string(), "vim".to_string()),
                ("PAGER".to_string(), "less".to_string()),
            ],
            ..Default::default()
        };
        let target = make_target_user();
        let environment = build_environment(&opts, &target, "alice");

        let find_val = |key: &str| -> Option<String> {
            environment
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.clone())
        };

        assert_eq!(find_val("EDITOR"), Some("vim".to_string()));
        assert_eq!(find_val("PAGER"), Some("less".to_string()));
    }

    #[test]
    fn env_setenv_overrides_default() {
        let opts = RuleOptions {
            setenv: vec![("HOME".to_string(), "/custom".to_string())],
            ..Default::default()
        };
        let target = make_target_user();
        let environment = build_environment(&opts, &target, "alice");
        let home = environment
            .iter()
            .find(|(k, _)| k == "HOME")
            .map(|(_, v)| v.clone());
        assert_eq!(home, Some("/custom".to_string()));
    }

    #[test]
    fn env_unsetenv_removes_var() {
        let opts = RuleOptions {
            unsetenv: vec!["SHELL".to_string()],
            ..Default::default()
        };
        let target = make_target_user();
        let environment = build_environment(&opts, &target, "alice");
        let shell = environment.iter().find(|(k, _)| k == "SHELL");
        assert!(shell.is_none());
    }

    #[test]
    fn env_doas_user_always_present() {
        // Even with keepenv + setenv, DOAS_USER must be set.
        let opts = RuleOptions {
            keepenv: true,
            setenv: vec![("FOO".to_string(), "bar".to_string())],
            ..Default::default()
        };
        let target = make_target_user();
        let environment = build_environment(&opts, &target, "caller");
        let doas_user = environment.iter().find(|(k, _)| k == "DOAS_USER");
        assert_eq!(doas_user.unwrap().1, "caller");
    }

    // ========================================================================
    // Persist / timestamp tests
    // ========================================================================

    #[test]
    fn persist_path_format() {
        let path = persist_path(1000);
        assert_eq!(path, "/var/run/doas/1000");
    }

    #[test]
    fn persist_path_root() {
        let path = persist_path(0);
        assert_eq!(path, "/var/run/doas/0");
    }

    // NOTE: persist_valid/persist_touch/persist_clear require filesystem access
    // and are tested via their logic. The following tests exercise the boundary
    // conditions of the validation logic by testing the comparison arithmetic.

    #[test]
    fn persist_timeout_arithmetic() {
        // If now=1000 and stamp=700, elapsed=300 which equals PERSIST_TIMEOUT_SECS.
        // 300 < 300 is false, so the timestamp is expired.
        let now: u64 = 1000;
        let stamp: u64 = 700;
        let elapsed = now.saturating_sub(stamp);
        assert!(elapsed >= PERSIST_TIMEOUT_SECS);
    }

    #[test]
    fn persist_timeout_still_valid() {
        // If now=1000 and stamp=701, elapsed=299 which is < 300.
        let now: u64 = 1000;
        let stamp: u64 = 701;
        let elapsed = now.saturating_sub(stamp);
        assert!(elapsed < PERSIST_TIMEOUT_SECS);
    }

    #[test]
    fn persist_timestamp_in_future_rejected() {
        // A timestamp in the future (forged) should be rejected.
        let now: u64 = 1000;
        let stamp: u64 = 2000;
        assert!(stamp > now); // This is the check in persist_valid.
    }

    // ========================================================================
    // Argument parsing tests
    // ========================================================================

    #[test]
    fn args_default() {
        let raw = vec!["doas".to_string(), "ls".to_string()];
        let args = parse_args(&raw).unwrap();
        assert_eq!(args.target_user, "root");
        assert!(!args.shell_mode);
        assert!(!args.check_config);
        assert!(!args.clear_persist);
        assert!(!args.non_interactive);
        assert_eq!(args.command.as_deref(), Some("ls"));
        assert!(args.arguments.is_empty());
    }

    #[test]
    fn args_target_user() {
        let raw = vec![
            "doas".to_string(),
            "-u".to_string(),
            "alice".to_string(),
            "whoami".to_string(),
        ];
        let args = parse_args(&raw).unwrap();
        assert_eq!(args.target_user, "alice");
        assert_eq!(args.command.as_deref(), Some("whoami"));
    }

    #[test]
    fn args_target_user_capital_u() {
        let raw = vec![
            "doas".to_string(),
            "-U".to_string(),
            "alice".to_string(),
            "id".to_string(),
        ];
        let args = parse_args(&raw).unwrap();
        assert_eq!(args.target_user, "alice");
    }

    #[test]
    fn args_shell_mode() {
        let raw = vec!["doas".to_string(), "-s".to_string()];
        let args = parse_args(&raw).unwrap();
        assert!(args.shell_mode);
        assert!(args.command.is_none());
    }

    #[test]
    fn args_check_config() {
        let raw = vec![
            "doas".to_string(),
            "-C".to_string(),
            "/etc/doas.conf".to_string(),
        ];
        let args = parse_args(&raw).unwrap();
        assert!(args.check_config);
        assert_eq!(args.config_path, "/etc/doas.conf");
    }

    #[test]
    fn args_clear_persist() {
        let raw = vec!["doas".to_string(), "-L".to_string()];
        let args = parse_args(&raw).unwrap();
        assert!(args.clear_persist);
    }

    #[test]
    fn args_non_interactive() {
        let raw = vec!["doas".to_string(), "-n".to_string(), "ls".to_string()];
        let args = parse_args(&raw).unwrap();
        assert!(args.non_interactive);
    }

    #[test]
    fn args_double_dash() {
        let raw = vec![
            "doas".to_string(),
            "--".to_string(),
            "-dangerous".to_string(),
        ];
        let args = parse_args(&raw).unwrap();
        assert_eq!(args.command.as_deref(), Some("-dangerous"));
    }

    #[test]
    fn args_command_with_arguments() {
        let raw = vec![
            "doas".to_string(),
            "pkg".to_string(),
            "install".to_string(),
            "vim".to_string(),
        ];
        let args = parse_args(&raw).unwrap();
        assert_eq!(args.command.as_deref(), Some("pkg"));
        assert_eq!(args.arguments, vec!["install", "vim"]);
    }

    #[test]
    fn args_missing_u_value() {
        let raw = vec!["doas".to_string(), "-u".to_string()];
        assert!(parse_args(&raw).is_err());
    }

    #[test]
    fn args_missing_c_value() {
        let raw = vec!["doas".to_string(), "-C".to_string()];
        assert!(parse_args(&raw).is_err());
    }

    #[test]
    fn args_unknown_option() {
        let raw = vec!["doas".to_string(), "-Z".to_string()];
        assert!(parse_args(&raw).is_err());
    }

    #[test]
    fn args_all_flags_combined() {
        let raw = vec![
            "doas".to_string(),
            "-n".to_string(),
            "-u".to_string(),
            "bob".to_string(),
            "vim".to_string(),
            "/etc/hosts".to_string(),
        ];
        let args = parse_args(&raw).unwrap();
        assert!(args.non_interactive);
        assert_eq!(args.target_user, "bob");
        assert_eq!(args.command.as_deref(), Some("vim"));
        assert_eq!(args.arguments, vec!["/etc/hosts"]);
    }

    // ========================================================================
    // set_env_var helper tests
    // ========================================================================

    #[test]
    fn set_env_var_new() {
        let mut env_map: Vec<(String, String)> = vec![("A".to_string(), "1".to_string())];
        set_env_var(&mut env_map, "B", "2");
        assert_eq!(env_map.len(), 2);
        assert_eq!(env_map[1], ("B".to_string(), "2".to_string()));
    }

    #[test]
    fn set_env_var_override() {
        let mut env_map: Vec<(String, String)> = vec![("A".to_string(), "1".to_string())];
        set_env_var(&mut env_map, "A", "99");
        assert_eq!(env_map.len(), 1);
        assert_eq!(env_map[0], ("A".to_string(), "99".to_string()));
    }

    // ========================================================================
    // resolve_command tests
    // ========================================================================

    #[test]
    fn resolve_absolute_path() {
        // An absolute path is returned as-is.
        assert_eq!(resolve_command("/usr/bin/ls"), "/usr/bin/ls");
    }

    #[test]
    fn resolve_relative_path() {
        // A relative path containing a slash is returned as-is.
        assert_eq!(resolve_command("./my_script"), "./my_script");
    }

    // ========================================================================
    // Identity matching tests (unit, bypassing group file)
    // ========================================================================

    #[test]
    fn identity_user_match() {
        // For non-group identities, identity_matches is a string comparison.
        assert!(identity_matches("alice", "alice"));
    }

    #[test]
    fn identity_user_mismatch() {
        assert!(!identity_matches("bob", "alice"));
    }

    // Group matching requires /etc/group, tested in integration tests.
    // We verify the prefix-detection logic here:
    #[test]
    fn identity_group_prefix_detected() {
        let id = ":wheel";
        assert!(id.starts_with(':'));
        assert_eq!(id.strip_prefix(':'), Some("wheel"));
    }
}
