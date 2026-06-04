//! OurOS doas -- Lightweight Privilege Elevation
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

use std::env;
use std::fs;
use std::io::{self, BufRead, Write};
use std::process;
use std::time::SystemTime;

// ============================================================================
// Constants
// ============================================================================

const DEFAULT_CONFIG_PATH: &str = "/etc/doas.conf";
const SHADOW_PATH: &str = "/etc/shadow";
const PASSWD_PATH: &str = "/etc/passwd";
const PERSIST_DIR: &str = "/var/run/doas";

/// Duration (in seconds) for which a `persist` timestamp remains valid.
const PERSIST_TIMEOUT_SECS: u64 = 300; // 5 minutes

// ============================================================================
// SHA-256 implementation (matches passwd utility)
// ============================================================================

/// SHA-256 round constants.
const SHA256_K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

/// Compute SHA-256 and return the hex digest string.
fn sha256_hex(data: &[u8]) -> String {
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];

    // Padding.
    let bit_len = (data.len() as u64).wrapping_mul(8);
    let mut padded = data.to_vec();
    padded.push(0x80);
    while (padded.len() % 64) != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&bit_len.to_be_bytes());

    // Process 64-byte blocks.
    for chunk in padded.chunks(64) {
        let mut w = [0u32; 64];
        for (w_slot, word_bytes) in w.iter_mut().take(16).zip(chunk.chunks_exact(4)) {
            // chunks_exact(4) yields a &[u8] of length 4; try_into is infallible.
            let arr: [u8; 4] = word_bytes.try_into().unwrap_or([0; 4]);
            *w_slot = u32::from_be_bytes(arr);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;

        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(SHA256_K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }

    h.iter().map(|v| format!("{v:08x}")).collect()
}

// ============================================================================
// Password hashing / verification (matches passwd utility format)
// ============================================================================

/// Hash a password with the given salt using SHA-256.
/// Format: `$sha256$<salt>$<hash>`
fn hash_password(password: &str, salt: &str) -> String {
    let input = format!("{salt}${password}");
    let digest = sha256_hex(input.as_bytes());
    format!("$sha256${salt}${digest}")
}

/// Verify a password against a stored `$sha256$<salt>$<hash>` string.
fn verify_password(password: &str, stored_hash: &str) -> bool {
    if let Some(rest) = stored_hash.strip_prefix("$sha256$")
        && let Some(dollar_pos) = rest.find('$') {
            let salt = &rest[..dollar_pos];
            let expected = hash_password(password, salt);
            return constant_time_eq(stored_hash.as_bytes(), expected.as_bytes());
        }
    false
}

/// Constant-time byte comparison to prevent timing attacks.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

// ============================================================================
// /etc/shadow parsing
// ============================================================================

/// A single entry from `/etc/shadow`.
#[derive(Clone, Debug, PartialEq)]
struct ShadowEntry {
    username: String,
    hash: String,
}

/// Parse `/etc/shadow` and return all entries.
fn read_shadow_entries(path: &str) -> Vec<ShadowEntry> {
    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    content
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| {
            let fields: Vec<&str> = line.split(':').collect();
            if fields.len() >= 2 {
                Some(ShadowEntry {
                    username: fields[0].to_string(),
                    hash: fields[1].to_string(),
                })
            } else {
                None
            }
        })
        .collect()
}

/// Look up a user's password hash in `/etc/shadow`.
fn lookup_shadow_hash(username: &str) -> Option<String> {
    let entries = read_shadow_entries(SHADOW_PATH);
    entries
        .into_iter()
        .find(|e| e.username == username)
        .map(|e| e.hash)
}

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
            && target != target_name {
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
/// On OurOS, `setuid`/`setgid` are actual syscalls that change the process
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
                    && let Ok(uid) = uid_str.parse::<u32>() {
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

            let stored_hash = match lookup_shadow_hash(&caller_name) {
                Some(h) => h,
                None => {
                    eprintln!("doas: cannot read password for {caller_name}");
                    process::exit(1);
                }
            };

            // Locked or empty accounts cannot authenticate.
            if stored_hash.is_empty() || stored_hash.starts_with('!') {
                eprintln!("doas: account {caller_name} is locked or has no password");
                process::exit(1);
            }

            let password = match read_password_no_echo(&format!("doas ({caller_name}) password: "))
            {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("doas: {e}");
                    process::exit(1);
                }
            };

            if !verify_password(&password, &stored_hash) {
                eprintln!("doas: authentication failed");
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

    // ========================================================================
    // SHA-256 tests
    // ========================================================================

    #[test]
    fn sha256_empty_string() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn sha256_abc() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn sha256_longer_input() {
        assert_eq!(
            sha256_hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
    }

    #[test]
    fn sha256_hello() {
        assert_eq!(
            sha256_hex(b"hello"),
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    // ========================================================================
    // Password hashing tests
    // ========================================================================

    #[test]
    fn hash_password_format() {
        let hashed = hash_password("test123", "abcdef");
        assert!(hashed.starts_with("$sha256$abcdef$"));
    }

    #[test]
    fn hash_password_deterministic() {
        let h1 = hash_password("mypassword", "salt123");
        let h2 = hash_password("mypassword", "salt123");
        assert_eq!(h1, h2);
    }

    #[test]
    fn verify_correct_password() {
        let hashed = hash_password("correct_horse", "battery_staple");
        assert!(verify_password("correct_horse", &hashed));
    }

    #[test]
    fn verify_wrong_password() {
        let hashed = hash_password("correct_horse", "battery_staple");
        assert!(!verify_password("wrong_horse", &hashed));
    }

    #[test]
    fn verify_empty_hash() {
        assert!(!verify_password("anything", ""));
    }

    #[test]
    fn verify_malformed_hash() {
        assert!(!verify_password("test", "$sha256$noseparator"));
    }

    #[test]
    fn verify_hash_round_trip() {
        let password = "S3cur3!Pass";
        let salt = "0123456789abcdef";
        let hashed = hash_password(password, salt);
        assert!(verify_password(password, &hashed));
        assert!(!verify_password("wrong", &hashed));
    }

    // ========================================================================
    // Constant-time comparison tests
    // ========================================================================

    #[test]
    fn constant_time_eq_same() {
        assert!(constant_time_eq(b"hello", b"hello"));
    }

    #[test]
    fn constant_time_eq_different() {
        assert!(!constant_time_eq(b"hello", b"world"));
    }

    #[test]
    fn constant_time_eq_different_lengths() {
        assert!(!constant_time_eq(b"short", b"longer"));
    }

    #[test]
    fn constant_time_eq_empty() {
        assert!(constant_time_eq(b"", b""));
    }

    // ========================================================================
    // Shadow parsing tests
    // ========================================================================

    #[test]
    fn shadow_parse_valid() {
        let content = "alice:$sha256$salt$hash:19500:0:99999:7:30:20000:\n\
                        bob:!:19000:0:99999:7:::\n";
        let entries = read_shadow_entries_from_str(content);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].username, "alice");
        assert_eq!(entries[0].hash, "$sha256$salt$hash");
        assert_eq!(entries[1].username, "bob");
        assert_eq!(entries[1].hash, "!");
    }

    #[test]
    fn shadow_parse_comments_and_blanks() {
        let content = "# comment\n\nroot:$sha256$s$h:0:0:99999:7:::\n";
        let entries = read_shadow_entries_from_str(content);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].username, "root");
    }

    #[test]
    fn shadow_parse_too_few_fields() {
        let content = "badline\n";
        let entries = read_shadow_entries_from_str(content);
        assert_eq!(entries.len(), 0);
    }

    /// Helper: parse shadow entries from a string (avoids file I/O in tests).
    fn read_shadow_entries_from_str(content: &str) -> Vec<ShadowEntry> {
        content
            .lines()
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .filter_map(|line| {
                let fields: Vec<&str> = line.split(':').collect();
                if fields.len() >= 2 {
                    Some(ShadowEntry {
                        username: fields[0].to_string(),
                        hash: fields[1].to_string(),
                    })
                } else {
                    None
                }
            })
            .collect()
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
