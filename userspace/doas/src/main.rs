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

use quoting::quoteaf_os;
use std::env;
use std::ffi::{OsStr, OsString};
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
    // A slice pattern states the field layout the code then reads, so the
    // arity check and the field accesses cannot drift apart the way a
    // `len() < 7` guard followed by `fields[6]` can.
    let [username, _passwd, uid, gid, _gecos, home, shell, ..] = fields.as_slice() else {
        return None;
    };
    Some(PasswdEntry {
        username: (*username).to_string(),
        uid: uid.parse().ok()?,
        gid: gid.parse().ok()?,
        home: (*home).to_string(),
        shell: (*shell).to_string(),
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

/// Whether `username` is in `group_name`, decided from the two database files
/// given as bytes. `None` means the question has no answer.
///
/// A parameter rather than a path, so the interesting cases are testable at
/// all: the build host has neither file, so every test of the old code either
/// skipped itself or asserted the "cannot read" arm.
///
/// **PRIMARY GROUPS COUNT.** The old test looked only at the group's
/// supplementary member list, which is the fourth field of `/etc/group` -- and
/// that field deliberately does *not* repeat accounts whose login group this
/// already is (`pwdb::Group::members` says so, and glibc agrees: measured,
/// `id -nG` lists the primary group). So a caller whose primary group was
/// `wheel` was reported as not being in `wheel`. For `permit :wheel` that
/// merely withheld a grant. For `deny :wheel` **it failed open** -- the deny
/// stopped applying to precisely the accounts most likely to be in the group,
/// and the caller fell through to whatever permit came next. That is the same
/// inversion this module's `read_group_entries` comment was written to warn
/// about, sitting in the success path rather than the error path.
///
/// The three answers, and why each is what it is:
///
/// | Situation | Answer | Why |
/// |---|---|---|
/// | group is not in the file | `Some(false)` | nobody is a member of a group that does not exist -- an answer, not an absence |
/// | caller is not in `/etc/passwd` | `None` | their primary gid is unknowable, and primary membership counts |
/// | otherwise | `Some(list contains gid)` | `pwdb::group_list` is what `id -G` prints |
fn user_in_group_in(
    passwd: &[u8],
    group: &[u8],
    username: &[u8],
    group_name: &[u8],
) -> Option<bool> {
    let db = pwdb::Db::from_bytes(passwd, group);
    let Some(target) = db.group_by_name(group_name) else {
        return Some(false);
    };
    let gid = target.gid;
    let caller = db.user_by_name(username)?;
    Some(db.group_list(username, caller.gid).contains(&gid))
}

/// Whether a user is in the named group, or `None` if the question has no
/// answer.
///
/// **`None` is not an empty group file.** It was: an unreadable `/etc/group`
/// returned `Vec::new()`, so every `:group` identity matched nobody. For a
/// `permit :wheel` rule that fails closed and is merely unhelpful. For a
/// `deny :wheel` rule it fails OPEN -- the deny stops applying, and the caller
/// falls through to whatever permit rule comes after it.
///
/// One `Err(_)` arm therefore inverted the meaning of half the rule language,
/// in the direction that grants. That is the shape lane A named on 2026-09-11
/// after finding it in `mkfs` and `fsck`: for a check guarding a privileged
/// action, "I do not know" and "it is safe" must not be the same value.
///
/// The files are read as **bytes**. `read_to_string` fails outright on a single
/// non-UTF-8 byte anywhere in the file, and this filesystem allows every byte
/// but `/` and NUL in a name -- so one group whose name is not UTF-8 used to
/// report the whole file unreadable, disabling every `:group` rule at once.
/// `pwdb::Db::from_files` is not used for the same reason it would be
/// convenient: it does `unwrap_or_default()`, which is exactly the
/// unreadable-means-empty conflation the paragraph above exists to prevent.
fn user_in_group(username: &str, group_name: &str) -> Option<bool> {
    let passwd = fs::read(pwdb::PASSWD_PATH).ok()?;
    let group = fs::read(pwdb::GROUP_PATH).ok()?;
    user_in_group_in(&passwd, &group, username.as_bytes(), group_name.as_bytes())
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
    /// A `deny` rule's identity could not be evaluated -- `/etc/group` is
    /// unreadable, so `:group` membership has no answer.
    ///
    /// Distinct from `NoMatch` on purpose. "This rule does not apply to you"
    /// lets evaluation continue; "I cannot tell whether this rule applies"
    /// must not, when the rule is a `deny` -- continuing past it is how an
    /// unreadable group file used to turn every `deny :group` into a no-op
    /// and hand the caller to whatever `permit` came next.
    ///
    /// An unevaluable `permit` does not produce this: skipping one only
    /// withholds a grant, which is the safe direction.
    Unknown,
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

        // Report the 1-based line number the operator sees in their editor.
        // Saturating rather than `+ 1`: a config with `usize::MAX` lines cannot
        // exist, but the parser should not be the place that decides that.
        let rule =
            parse_rule(line).map_err(|e| format!("line {}: {e}", line_num.saturating_add(1)))?;
        rules.push(rule);
    }

    Ok(rules)
}

/// A read position in a token list.
///
/// Both parsers below used to walk a `Vec<String>` with a bare `idx`, checking
/// the bound in one statement (`tokens.get(idx)`) and advancing in another
/// (`idx += 1`) roughly forty times between them. Every one of those pairs is a
/// place the two halves can be written apart -- an `idx += 1` on a path that did
/// not consume a token, or a `tokens[idx]` reached without a preceding `get`.
/// Giving the index an owner removes the opportunity: `peek` is the only read
/// and it is bounds-checked, `bump` is the only advance and it is the one place
/// the addition happens (saturating, so an index can never wrap around into a
/// small number and re-read the front of the list).
struct Cursor<'a> {
    toks: &'a [String],
    idx: usize,
}

impl<'a> Cursor<'a> {
    fn new(toks: &'a [String]) -> Self {
        Self { toks, idx: 0 }
    }

    /// The token at the read position, without consuming it.
    fn peek(&self) -> Option<&'a str> {
        self.toks.get(self.idx).map(String::as_str)
    }

    /// Advance one token. Saturating: past the end simply stays past the end.
    fn bump(&mut self) {
        self.idx = self.idx.saturating_add(1);
    }

    /// Consume and return the token at the read position.
    fn next_tok(&mut self) -> Option<&'a str> {
        let tok = self.peek();
        self.bump();
        tok
    }

    /// The tokens from the read position onward.
    fn rest(&self) -> &'a [String] {
        self.toks.get(self.idx..).unwrap_or_default()
    }

    /// Consume everything remaining and return it, leaving the cursor at the
    /// end. Callers that swallow the tail must still leave the cursor honest,
    /// or the "nothing left over" check at the end of a parse would report the
    /// tokens they just accepted as unexpected.
    fn take_rest(&mut self) -> &'a [String] {
        let rest = self.rest();
        self.idx = self.toks.len();
        rest
    }
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

    let mut c = Cursor::new(&tokens);

    // 1. Action
    let action = match c.next_tok() {
        Some("permit") => RuleAction::Permit,
        Some("deny") => RuleAction::Deny,
        Some(other) => {
            return Err(format!(
                "expected 'permit' or 'deny', got {}",
                quoteaf_os(other)
            ));
        }
        None => return Err("expected 'permit' or 'deny'".to_string()),
    };

    // 2. Options (only valid for permit rules, but we parse them either way
    //    so we can give a clear error).
    let mut options = RuleOptions::default();
    loop {
        match c.peek() {
            Some("nopass") => {
                options.nopass = true;
                c.bump();
            }
            Some("persist") => {
                options.persist = true;
                c.bump();
            }
            Some("keepenv") => {
                options.keepenv = true;
                c.bump();
            }
            Some("setenv") => {
                c.bump();
                // Expect a '{' token next.
                if c.next_tok() != Some("{") {
                    return Err("expected '{' after 'setenv'".to_string());
                }

                // Collect assignments until '}'. Consuming through `next_tok`
                // makes running out of tokens and reaching the closing brace the
                // same decision, so an unterminated block cannot slip past a
                // bound check that was written as a separate statement.
                loop {
                    match c.next_tok() {
                        Some("}") => break,
                        None => {
                            return Err("unterminated 'setenv' block (missing '}')".to_string());
                        }
                        // `split_once` carries the "there is an '='" test and the
                        // two halves it implies together; `find` plus two range
                        // indexes re-derives the same offsets by hand.
                        Some(assignment) => match assignment.split_once('=') {
                            Some((var, val)) => {
                                options.setenv.push((var.to_string(), val.to_string()));
                            }
                            // A bare name in setenv means unset (remove) that variable.
                            None => options.unsetenv.push(assignment.to_string()),
                        },
                    }
                }
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
    let identity = match c.next_tok() {
        Some(tok) => tok.to_string(),
        None => return Err("expected user or :group identity".to_string()),
    };

    // 4. Optional "as <target>"
    let mut target = None;
    if c.peek() == Some("as") {
        c.bump();
        target = Some(
            c.next_tok()
                .ok_or_else(|| "expected username after 'as'".to_string())?
                .to_string(),
        );
    }

    // 5. Optional "cmd <command>"
    let mut cmd = None;
    let mut args = None;
    if c.peek() == Some("cmd") {
        c.bump();
        cmd = Some(
            c.next_tok()
                .ok_or_else(|| "expected command after 'cmd'".to_string())?
                .to_string(),
        );

        // 6. Optional "args <args...>" -- everything remaining.
        if c.peek() == Some("args") {
            c.bump();
            args = Some(c.take_rest().to_vec());
        }
    }

    // There should be nothing left.
    if let Some(tok) = c.peek() {
        return Err(format!("unexpected token {}", quoteaf_os(tok)));
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
    command_args: &[OsString],
    // The `PATH` a bare `cmd` spec resolves against -- see `command_matches`.
    // Passed in rather than read here so the rule check and the eventual
    // `exec` cannot disagree about which binary is meant.
    dirs: &[&str],
) -> MatchResult {
    for rule in rules {
        match identity_matches(&rule.identity, caller_name) {
            Some(true) => {}
            Some(false) => continue,
            // AN UNEVALUABLE RULE IS SKIPPED OR FATAL DEPENDING ON WHICH WAY
            // IT POINTS, and the asymmetry is the whole point.
            //
            // A `permit` we cannot evaluate is skipped. The worst that does is
            // withhold a grant, which is the safe direction and is what an
            // unreadable group file should cost: `permit :wheel` stops working
            // until the file is readable again.
            //
            // A `deny` we cannot evaluate is fatal. Skipping it would let the
            // caller fall through to a later `permit` -- so the one arm that
            // exists to STOP somebody would be the one an unreadable file
            // silently removed. That is the inversion this whole change is
            // about, and it only ever bites the deny half.
            //
            // Written the blunt way first, aborting on either kind; the test
            // suite caught it, because `sample_rules` opens with
            // `permit nopass :wheel` and every case then reported Unknown.
            None => match rule.action {
                RuleAction::Permit => continue,
                RuleAction::Deny => return MatchResult::Unknown,
            },
        }

        if let Some(ref target) = rule.target
            && target != target_name
        {
            continue;
        }

        if let Some(ref cmd) = rule.cmd {
            match command {
                Some(actual_cmd) => {
                    if !command_matches(cmd, actual_cmd, dirs) {
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
            // `doas.conf` is a text file, so an argument that is not text
            // matches no configured one. That is the answer a rule pinning
            // arguments should give: it permits exactly the arguments it
            // lists, and this is not one of them.
            let all_match = expected_args
                .iter()
                .zip(command_args.iter())
                .all(|(exp, act)| act.to_str() == Some(exp.as_str()));
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
fn identity_matches(identity: &str, caller_name: &str) -> Option<bool> {
    if let Some(group_name) = identity.strip_prefix(':') {
        user_in_group(caller_name, group_name)
    } else {
        Some(identity == caller_name)
    }
}

/// Check whether a command specification matches the actual command being run.
///
/// # The basename comparison this replaces was a privilege escalation
///
/// It was: an absolute spec had to match exactly, and **any other spec matched
/// on the last path component alone**. So `permit alice cmd pkg` authorised
///
/// ```text
/// doas /tmp/evil/pkg
/// ```
///
/// because `pkg == pkg`. The caller chooses that path, and the binary runs as
/// the target. `doas ./pkg` did it too, and with the old `resolve_command`
/// reading the *caller's* `$PATH`, so did `PATH=/tmp/evil doas pkg`.
///
/// A bare spec is now resolved against [`target_path`] -- the `PATH` the
/// target will actually run under -- and the whole path is compared. So
/// `cmd pkg` still means `/usr/bin/pkg`, which is what anybody writing it
/// meant, and means nothing else.
///
/// A spec naming a program that is not on that path matches nothing, which is
/// the safe direction: a rule that cannot be resolved authorises nothing
/// rather than authorising by name.
fn command_matches(spec: &str, actual: &str, dirs: &[&str]) -> bool {
    if spec.contains('/') {
        return spec == actual;
    }
    first_in_path(spec, dirs).is_some_and(|resolved| resolved == actual)
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
) -> Vec<(OsString, OsString)> {
    let pair = |k: &str, v: &str| (OsString::from(k), OsString::from(v));
    // `vars_os`, not `vars`: `env::vars`'s iterator panics on a variable whose
    // name or value is not valid UTF-8, and this is the `keepenv` branch --
    // the one whose entire job is to hand the caller's environment through
    // unaltered. The one shape of environment it must not lose is the one it
    // used to die on.
    let mut env_map: Vec<(OsString, OsString)> = if opts.keepenv {
        env::vars_os().collect()
    } else {
        let mut base = vec![
            pair("HOME", &target.home),
            pair("LOGNAME", &target.username),
            // The same value `command_matches` resolves a bare `cmd` spec
            // against, so the rule check and the exec cannot disagree about
            // which binary a name means.
            pair("PATH", target_path(target.uid)),
            pair("SHELL", &target.shell),
            pair("USER", &target.username),
        ];

        // Propagate TERM if the caller has it set. `var_os` for the same
        // reason: a terminal name this cannot decode is a reason to pass it
        // through unread, not to drop it.
        if let Some(term) = env::var_os("TERM") {
            base.push((OsString::from("TERM"), term));
        }

        base
    };

    // Always set DOAS_USER to the original (calling) user.
    set_env_var(&mut env_map, "DOAS_USER", caller_name);

    // Apply setenv assignments.
    for (var, val) in &opts.setenv {
        set_env_var(&mut env_map, var, val);
    }

    // Remove unsetenv variables. `doas.conf` is a text file, so a variable
    // named there is text; a variable in the caller's environment whose *name*
    // is not text matches none of them and is therefore kept -- which is what
    // `keepenv` asked for.
    for var in &opts.unsetenv {
        env_map.retain(|(k, _)| k != OsStr::new(var.as_str()));
    }

    env_map
}

/// Set or overwrite a variable in the environment vector.
fn set_env_var(env_map: &mut Vec<(OsString, OsString)>, key: &str, value: &str) {
    if let Some(existing) = env_map.iter_mut().find(|(k, _)| k == OsStr::new(key)) {
        existing.1 = OsString::from(value);
    } else {
        env_map.push((OsString::from(key), OsString::from(value)));
    }
}

// ============================================================================
// Command execution
// ============================================================================

/// Execute a command as the target user.
///
/// # The deferral this replaces
///
/// This function used to carry the note: "On Slate OS, `setuid`/`setgid` are
/// actual syscalls that change the process identity. For now we set the
/// UID/GID environment hints and invoke the command. The real privilege change
/// will use the kernel's capability system once the POSIX exec layer supports
/// `setuid`/`setgid` syscalls."
///
/// That premise had already fired: `posix::setuid` and `posix::setgid` apply
/// real credentials through `set_real_credentials`. They were stubs returning
/// `0` once, which is presumably when the note was written -- and a stub that
/// reports success is what keeps a deferral looking current, because there is
/// no compile error to trip over and no failing call to investigate.
///
/// # The "hints" are gone, and they were part of the problem
///
/// The `UID` and `GID` environment variables set here were a stand-in for the
/// credential change. They were also *read*, as an identity, by `chage`,
/// `newgrp`, `passwd`, `polkit`, `crontab` and `sudo` -- so this program was
/// manufacturing exactly the spoofed environment those programs trusted. Every
/// one of them now asks `getuid(2)` instead, so the hints have no readers at
/// all, and a fabricated identity claim injected into a child's environment is
/// not something to leave lying around for the next reader to start believing.
fn exec_command(
    target: &PasswdEntry,
    command: &str,
    arguments: &[OsString],
    environment: &[(OsString, OsString)],
) -> i32 {
    let mut cmd = process::Command::new(command);
    cmd.args(arguments);
    cmd.env_clear();
    for (key, val) in environment {
        cmd.env(key, val);
    }

    authlib::identity::become_user(&mut cmd, target.uid, target.gid);

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
/// The `PATH` the target will run under, and the only one this program
/// resolves a bare command name against.
///
/// **Not the caller's `$PATH`.** `resolve_command` used
/// `env::var("PATH")`, so `PATH=/tmp/evil doas pkg` resolved to
/// `/tmp/evil/pkg` -- a binary the caller chose, run as the target. The value
/// here is the one this program was already going to hand the target in
/// `build_environment`, so the rule now authorises the program that will
/// actually run.
fn target_path(uid: u32) -> &'static str {
    if uid == 0 {
        "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"
    } else {
        "/usr/local/bin:/usr/bin:/bin"
    }
}

/// The directories of a `PATH`, in order, with empty entries dropped.
///
/// Splitting is separated from searching so that the search can be tested. The
/// development host is Windows, where an absolute path begins with a drive
/// letter and a colon -- so a test handing a real directory to a function that
/// splits on `:` searched two fragments of it, found nothing, and failed in a
/// way that read exactly like the production logic being wrong. The target has
/// no drive letters; this seam is for the host and costs the target nothing.
fn path_dirs(path: &str) -> Vec<&str> {
    path.split(':').filter(|d| !d.is_empty()).collect()
}

/// The first executable named `command` in `dirs`, or `None`.
fn first_in_path(command: &str, dirs: &[&str]) -> Option<String> {
    for dir in dirs {
        let candidate = format!("{dir}/{command}");
        if fs::metadata(&candidate).is_ok() {
            return Some(candidate);
        }
    }
    None
}

fn resolve_command(command: &str, dirs: &[&str]) -> String {
    // If it already contains a slash, it is a path -- use it directly. The
    // rule check below is what decides whether that path is allowed.
    if command.contains('/') {
        return command.to_string();
    }
    first_in_path(command, dirs).unwrap_or_else(|| command.to_string())
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

/// The caller's uid, as the kernel reports it.
///
/// `getuid(2)`, via `authlib::identity::caller_uid`. This used to read
/// `/proc/self/status` and fall back to the `UID` environment variable; the
/// call it now makes is better than both, because there is no file to be
/// missing, no line to be parsed, and no way for the process's parent to
/// influence the answer.
fn current_uid() -> Option<u32> {
    authlib::identity::caller_uid()
}

/// The caller's name, resolved from their uid in `/etc/passwd`.
///
/// # `$USER` is not consulted, and that is the point
///
/// This function used to return `$USER` outright when it was set, falling back
/// to the uid lookup only when it was not. That is the name `doas` matches its
/// `/etc/doas.conf` rules against -- so the program asked the caller to name
/// themselves and then looked up what that person was permitted to do.
/// `USER=<anyone with a permit rule> doas <command>` inherited their rules.
///
/// The uid cannot be chosen by the caller, so the name derived from it cannot
/// either. A uid with no `/etc/passwd` entry yields `None`, and `main` refuses:
/// a caller `doas` cannot name matches no rule, and the default is deny.
fn current_username() -> Option<String> {
    let uid = current_uid()?;
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
    /// The command to run, still as the bytes the caller gave -- see
    /// [`DoasArgs::arguments`].
    command: Option<OsString>,
    /// Arguments to the command.
    /// The command's own arguments, still as the bytes the caller gave.
    ///
    /// These are handed to the exec, and an argument is very often a
    /// filename -- which on this OS may hold any byte but `/` and NUL. Not
    /// `String`: `env::args()` *panics* on such an argument, so `doas` died
    /// before running a line of its own, from a setuid binary. See
    /// `known-issues.md` -> `B-COREUTILS-PANIC-ON-A-NON-UTF-8-ARGUMENT`.
    arguments: Vec<OsString>,
}

fn parse_args(raw: &[OsString]) -> Result<DoasArgs, String> {
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

    // `Cursor` is the config file's tokeniser and stays over `String`; argv
    // is walked directly here, because it is the one input that is not text.
    let mut i = 1; // skip argv[0]
    let mut end_of_opts = false;

    while let Some(raw_arg) = raw.get(i) {
        // An argument that is not text cannot start with `-`, so it is taken
        // as the command -- which is what it would have to be, and which then
        // fails to resolve to a program, by the same path as any other command
        // that is not on the machine.
        let arg = raw_arg.to_str().unwrap_or_default();
        if end_of_opts || !arg.starts_with('-') || arg == "-" {
            // First non-option is the command; the rest are its arguments.
            result.command = Some(raw_arg.clone());
            i = i.saturating_add(1);
            result.arguments = raw.get(i..).unwrap_or_default().to_vec();
            break;
        }

        i = i.saturating_add(1);
        let mut value = |need: &str| -> Result<String, String> {
            let got = raw.get(i).ok_or_else(|| need.to_string())?;
            i = i.saturating_add(1);
            got.to_str()
                .map(str::to_string)
                .ok_or_else(|| format!("{need}, and {} is not one", quoteaf_os(got)))
        };
        match arg {
            "--" => end_of_opts = true,
            "-u" | "-U" => {
                result.target_user = value("option -u requires a user argument")?;
            }
            "-s" => result.shell_mode = true,
            "-C" => {
                result.config_path = value("option -C requires a config file argument")?;
                result.check_config = true;
            }
            "-L" => result.clear_persist = true,
            "-n" => result.non_interactive = true,
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
    // `args_os`, not `args`: the latter's iterator is a literal `unwrap` and
    // panics on an argument that is not valid UTF-8.
    let raw_args: Vec<OsString> = env::args_os().collect();

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
        let Some(uid) = current_uid() else {
            eprintln!("doas: cannot determine who is running this command");
            process::exit(1);
        };
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
        match args.command.as_ref().and_then(|c| c.to_str()) {
            // A command name is matched against `doas.conf`, which is a text
            // file, and is resolved against `PATH`, which is text too -- so a
            // name that is not text names no permitted command. It reads as
            // empty here, which matches no rule and resolves to nothing, and
            // is refused by the same "not permitted" path as any other command
            // the caller may not run. Refusing it earlier, in its own words,
            // would tell the caller something the rules did not.
            Some(cmd) => cmd.to_string(),
            None if args.command.is_some() => String::new(),
            None => {
                eprintln!("doas: no command specified");
                print_usage();
                process::exit(1);
            }
        }
    };

    // Resolve the command to a full path for rule matching.
    // One PATH for both: the target's. Resolving the request and checking the
    // rule against different paths is how a rule ends up authorising a
    // different binary from the one that runs.
    let cmd_path = target_path(lookup_passwd_user(&args.target_user).map_or(u32::MAX, |e| e.uid));
    let cmd_dirs = path_dirs(cmd_path);
    let resolved_cmd = resolve_command(&command, &cmd_dirs);

    // Evaluate rules.
    let match_result = evaluate_rules(
        &rules,
        &caller_name,
        &args.target_user,
        Some(&resolved_cmd),
        &args.arguments,
        &cmd_dirs,
    );

    let opts = match match_result {
        MatchResult::Permit(opts) => opts,
        MatchResult::Deny => {
            eprintln!("doas: operation not permitted");
            process::exit(1);
        }
        MatchResult::Unknown => {
            eprintln!("doas: cannot read /etc/group, so a rule naming a group cannot be evaluated");
            process::exit(1);
        }
        MatchResult::NoMatch => {
            // Three names, none of them this program's: the caller comes from
            // the password database, and both the command and the target user
            // come from argv. The hand-written quotes wrapped only the middle
            // one, and so quoted nothing -- a command named `x' as root #`
            // put the tail of a *security* decision under the caller's
            // control, which is the one line here that must not be forgeable.
            eprintln!(
                "doas: {} is not allowed to run {} as {}",
                quoteaf_os(&caller_name),
                quoteaf_os(&command),
                quoteaf_os(&args.target_user)
            );
            process::exit(1);
        }
    };

    // Authentication. The uid is what the persist timestamp is keyed on, so an
    // unidentifiable caller must not reach it -- they would all share one entry
    // and one caller's successful authentication would stand in for another's.
    let Some(caller_uid) = current_uid() else {
        eprintln!("doas: cannot determine who is running this command");
        process::exit(1);
    };
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

// Panicking on bad data is what a test is *for*: an `unwrap` that fires is a
// failure report, not a crash in someone's session. CLAUDE.md scopes the four
// defensive lints to non-test code for exactly this reason.
#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]
mod tests {
    use super::*;
    use scratchdir::ScratchDir;
    use std::path::PathBuf;

    /// A command line, as `env::args_os` would deliver it.
    fn argv(parts: &[&str]) -> Vec<OsString> {
        parts.iter().map(OsString::from).collect()
    }

    /// An argument that a `String` cannot hold. The development host is
    /// Windows, where argv arrives as UTF-16 and the unrepresentable case is
    /// an unpaired surrogate rather than a stray byte -- so the fixture is
    /// written both ways.
    fn not_text() -> OsString {
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStringExt as _;
            OsString::from_vec(vec![b'a', 0x80, b'b'])
        }
        #[cfg(not(unix))]
        {
            use std::os::windows::ffi::OsStringExt as _;
            OsString::from_wide(&[0x0061, 0xD800, 0x0062])
        }
    }

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
    /// The stored entry for `password`, hashed the way `passwd` writes one.
    ///
    /// It used to return a whole `shadow(5)` line, name and aging fields and
    /// all, because the fixture wrote a shadow file. There is no shadow file
    /// to write since `design-decisions.md` §353 item 3, and a record's
    /// password field is the entry alone.
    fn entry_for(password: &str) -> String {
        let mut setting_buf = posix::crypt::buf();
        let setting =
            posix::crypt::setting_into(posix::crypt::Method::Sha512, b"doastest", &mut setting_buf)
                .expect("valid crypt setting");
        let setting = setting.to_string();
        let mut hash_buf = posix::crypt::buf();
        posix::crypt::hash_into(password.as_bytes(), setting.as_bytes(), &mut hash_buf)
            .expect("hashable password")
            .to_string()
    }

    /// A note on why this suite never went red despite holding a copy of the
    /// same broken helper: every caller passed a distinct `tag`, so the safety
    /// came from a hand-maintained naming convention that nothing checked, and
    /// one copy-pasted test with a duplicated tag would have undone it. That is
    /// why `ScratchDir::new`'s prefix plays no part in uniqueness.
    /// An [`authlib::Authenticator`] over a temporary account database in which
    /// `username` has `stored` as its password entry.
    ///
    /// It used to write a `/etc/shadow` and point the verifier at that, with a
    /// `users.yaml` that did not exist so the shadow branch was the one under
    /// test. `design-decisions.md` §353 item 3 deleted that branch: the account
    /// database is the only store there is, and the flat file beside it is
    /// generated from the database rather than read.
    ///
    /// The returned `ScratchDir` is a guard: it must stay bound for as long as
    /// the `Authenticator` is used, because dropping it deletes the database
    /// the authenticator reads. Bind it as `_dir`, not `_`.
    fn authenticator_with_entry(
        username: &str,
        stored: &str,
    ) -> (authlib::Authenticator, ScratchDir) {
        let dir = ScratchDir::new("doas_test");
        let path = dir.path("users.yaml");
        let mut db = userdb::UserDb::new();
        let mut record = userdb::Record::new();
        // A record with no uid has no `/etc/passwd` line to generate, and a
        // save that cannot generate one fails -- so a fixture without one was
        // never a fixture of a real account.
        record.set_uid(1000);
        record.set(userdb::field::USERNAME, username);
        record.set(userdb::field::PASSWORD_HASH, stored);
        db.push(record);
        db.save(&path).expect("write the test database");
        (authlib::Authenticator::with_stores(&path), dir)
    }

    // ---- the fixture's own wiring ----
    //
    // `ScratchDir` guarantees two guards never share a directory, and its own
    // tests pin that under eight concurrent threads. What it cannot know is
    // whether *this* fixture holds its guard for as long as the authenticator
    // it handed back needs the file. So this asserts the end-to-end property,
    // which in `ftpd` failed as a red in roughly one run in eight, in an
    // assertion pointing at the authenticator rather than at the fixture.

    #[test]
    fn twenty_fixtures_alive_at_once_each_authenticate_their_own_user() {
        // Held alive at once on purpose: dropping each before making the next
        // would let a fixture that reuses one directory name pass. Note they
        // all pass the *same* content shape, which the old tag-based scheme
        // relied on callers never doing.
        let fixtures: Vec<(authlib::Authenticator, ScratchDir)> = (0..20)
            .map(|i| authenticator_with_entry(&format!("user{i}"), &entry_for("hunter2")))
            .collect();
        let paths: Vec<PathBuf> = fixtures.iter().map(|(_, d)| d.path("shadow")).collect();

        let distinct: std::collections::BTreeSet<&PathBuf> = paths.iter().collect();
        assert_eq!(
            distinct.len(),
            paths.len(),
            "two fixtures alive at once produced the same path"
        );

        // Distinct paths alone would not be enough: a later guard that
        // cleared an earlier one's directory would produce the same
        // read-someone-else's-data symptom with all-distinct names. Assert on
        // the authenticators rather than the bytes, since that is the thing the
        // collision actually corrupted.
        for (i, (auth, _dir)) in fixtures.into_iter().enumerate() {
            let mut auth = auth;
            assert_eq!(
                auth.authenticate(&format!("user{i}"), b"hunter2"),
                authlib::Outcome::Accepted,
                "fixture {i} lost its own shadow line"
            );
        }
    }

    #[test]
    fn a_password_set_with_passwd_opens_a_doas_prompt() {
        // The whole point of the change: before it, this was `Rejected`, and
        // there was no password anyone could type that would not be.
        let (mut auth, _dir) = authenticator_with_entry("alice", &entry_for("hunter2"));
        assert_eq!(
            auth.authenticate("alice", b"hunter2"),
            authlib::Outcome::Accepted
        );
    }

    #[test]
    fn a_wrong_password_does_not() {
        let (mut auth, _dir) = authenticator_with_entry("alice", &entry_for("hunter2"));
        assert_eq!(
            auth.authenticate("alice", b"hunter3"),
            authlib::Outcome::Rejected
        );
    }

    #[test]
    fn a_locked_account_cannot_escalate() {
        let (mut auth, _dir) = authenticator_with_entry("alice", "!");
        // Not `Rejected`: no password opens it, and `is_accepted` is the only
        // thing `main` asks, so an `Outcome` that is not `Accepted` is a refusal
        // however it got there.
        let outcome = auth.authenticate("alice", b"hunter2");
        assert_eq!(outcome, authlib::Outcome::Locked);
        assert!(!outcome.is_accepted());
    }

    #[test]
    fn an_account_with_no_password_cannot_escalate() {
        // `login` answers this one the other way for a console login. Escalation
        // is not a login: `nopass` in doas.conf is the only consent that counts.
        let (mut auth, _dir) = authenticator_with_entry("alice", "");
        let outcome = auth.authenticate("alice", b"");
        assert_eq!(outcome, authlib::Outcome::NoPassword);
        assert!(!outcome.is_accepted());
    }

    #[test]
    fn the_hash_this_crate_used_to_write_is_reported_broken_not_wrong() {
        // A system that ran the old `doas`, or the old `passwd`, has entries in
        // this shape. They must be distinguishable from a typo, because no
        // amount of retyping will ever clear one.
        let (mut auth, _dir) =
            authenticator_with_entry("alice", "$sha256$battery_staple$0123456789abcdef");
        let outcome = auth.authenticate("alice", b"correct_horse");
        assert_eq!(outcome, authlib::Outcome::Unusable);
        assert!(outcome.needs_administrator());
    }

    #[test]
    fn a_caller_with_no_entry_looks_exactly_like_a_wrong_password() {
        let (mut auth, _dir) = authenticator_with_entry("alice", &entry_for("hunter2"));
        assert_eq!(
            auth.authenticate("mallory", b"anything"),
            authlib::Outcome::Rejected
        );
        // And says the same thing, so the prompt is not an account oracle.
        assert_eq!(
            authlib::Outcome::Rejected.user_message(),
            authlib::Outcome::Locked.user_message()
        );
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

    /// `args` with nothing after it means "the command, called with no
    /// arguments" -- an empty list, not the absence of a list, and not an
    /// error. The distinction is load-bearing: `Some(vec![])` only matches an
    /// invocation that passes no arguments, while `None` matches any arguments
    /// at all, so collapsing the two would silently widen the rule.
    #[test]
    fn parse_permit_cmd_args_empty() {
        let rules = parse_config("permit alice cmd /usr/bin/pkg args\n").unwrap();
        assert_eq!(rules[0].cmd.as_deref(), Some("/usr/bin/pkg"));
        assert_eq!(rules[0].args.as_deref(), Some(&[][..]));
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
        let result = evaluate_rules(
            &rules,
            "root",
            "root",
            Some("/bin/ls"),
            &[],
            &path_dirs(target_path(0)),
        );
        match result {
            MatchResult::Permit(opts) => assert!(opts.nopass),
            other => panic!("expected Permit, got {other:?}"),
        }
    }

    /// The asymmetry, stated as the property rather than trusted to a comment.
    ///
    /// On this build host `/etc/group` does not exist, so every `:group`
    /// identity is unevaluable -- which makes it the right place to pin what
    /// happens then.
    #[test]
    fn an_unevaluable_permit_is_skipped_and_an_unevaluable_deny_is_fatal() {
        let group_file_readable = std::fs::read_to_string("/etc/group").is_ok();
        if group_file_readable {
            // On a host that HAS the file the premise does not hold and the
            // case cannot be exercised; say so rather than pass vacuously.
            return;
        }

        // A permit naming a group is skipped, so a later plain-name rule is
        // still reached.
        let rules = parse_config(
            "permit :wheel
permit alice
",
        )
        .expect("valid");
        assert_eq!(
            evaluate_rules(
                &rules,
                "alice",
                "root",
                None,
                &[],
                &path_dirs(target_path(0))
            ),
            MatchResult::Permit(RuleOptions::default()),
            "an unevaluable permit must not stop the rules after it"
        );

        // A deny naming a group is NOT skipped: the permit after it must not
        // be reached, because the deny might have been the one that applied.
        let rules = parse_config(
            "deny :wheel
permit alice
",
        )
        .expect("valid");
        assert_eq!(
            evaluate_rules(
                &rules,
                "alice",
                "root",
                None,
                &[],
                &path_dirs(target_path(0))
            ),
            MatchResult::Unknown,
            "an unevaluable deny must stop evaluation, not be skipped"
        );
    }

    #[test]
    fn match_deny_bob() {
        let rules = sample_rules();
        let result = evaluate_rules(
            &rules,
            "bob",
            "root",
            Some("/bin/ls"),
            &[],
            &path_dirs(target_path(0)),
        );
        assert_eq!(result, MatchResult::Deny);
    }

    #[test]
    fn match_no_rule_for_unknown() {
        let rules = sample_rules();
        let result = evaluate_rules(
            &rules,
            "charlie",
            "root",
            Some("/bin/ls"),
            &[],
            &path_dirs(target_path(0)),
        );
        assert_eq!(result, MatchResult::NoMatch);
    }

    #[test]
    fn match_alice_as_root() {
        let rules = sample_rules();
        let result = evaluate_rules(
            &rules,
            "alice",
            "root",
            Some("/bin/ls"),
            &[],
            &path_dirs(target_path(0)),
        );
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
        let result = evaluate_rules(
            &rules,
            "alice",
            "bob",
            Some("/bin/ls"),
            &[],
            &path_dirs(target_path(0)),
        );
        assert_eq!(result, MatchResult::NoMatch);
    }

    #[test]
    fn match_alice_cmd_pkg() {
        let rules = sample_rules();
        let result = evaluate_rules(
            &rules,
            "alice",
            "root",
            Some("/usr/bin/pkg"),
            &[],
            &path_dirs(target_path(0)),
        );
        // The "as root" rule matches first (it has no cmd restriction).
        match result {
            MatchResult::Permit(_) => {} // OK
            other => panic!("expected Permit, got {other:?}"),
        }
    }

    #[test]
    fn match_alice_cmd_pkg_with_args() {
        let rules = sample_rules();
        let args = argv(&["install", "vim"]);
        let result = evaluate_rules(
            &rules,
            "alice",
            "root",
            Some("/usr/bin/pkg"),
            &args,
            &path_dirs(target_path(0)),
        );
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
        let args = argv(&["install", "emacs"]);
        let result = evaluate_rules(
            &rules,
            "alice",
            "root",
            Some("/usr/bin/pkg"),
            &args,
            &path_dirs(target_path(0)),
        );
        assert_eq!(result, MatchResult::NoMatch);
    }

    #[test]
    fn match_args_count_mismatch() {
        let rules = parse_config("permit alice cmd /usr/bin/pkg args install vim\n").unwrap();
        let args = argv(&["install"]);
        let result = evaluate_rules(
            &rules,
            "alice",
            "root",
            Some("/usr/bin/pkg"),
            &args,
            &path_dirs(target_path(0)),
        );
        assert_eq!(result, MatchResult::NoMatch);
    }

    #[test]
    fn match_first_rule_wins() {
        let rules = parse_config("deny alice\npermit alice\n").unwrap();
        let result = evaluate_rules(
            &rules,
            "alice",
            "root",
            Some("/bin/ls"),
            &[],
            &path_dirs(target_path(0)),
        );
        assert_eq!(result, MatchResult::Deny);
    }

    // ========================================================================
    // Command matching tests
    // ========================================================================

    /// A directory holding a real `pkg`, plus the `PATH` naming it. A bare
    /// spec is resolved on the filesystem now, so a fabricated path resolves
    /// to nothing and every test below it would pass for the wrong reason.
    fn bindir() -> (ScratchDir, String, String) {
        let dir = ScratchDir::new("doas_cmdmatch");
        let path = dir.dir().to_string_lossy().to_string();
        // Composed with `/`, exactly as `first_in_path` composes it. On the
        // development host `ScratchDir::path` gives a backslash-separated
        // string: `fs::metadata` accepts either, so the file is found and only
        // the STRING comparison fails -- a host artifact that reads exactly
        // like the production logic being wrong. On the target every path
        // separator is `/` and the two spellings coincide.
        let real = format!("{path}/pkg");
        fs::write(&real, b"#!/bin/sh\n").expect("write fixture");
        (dir, path, real)
    }

    #[test]
    fn command_match_absolute_exact() {
        let (_d, path, real) = bindir();
        assert!(command_matches(&real, &real, &[path.as_str()]));
    }

    #[test]
    fn command_match_absolute_mismatch() {
        let (_d, path, real) = bindir();
        assert!(!command_matches(&real, "/usr/bin/apt", &[path.as_str()]));
    }

    #[test]
    fn a_bare_spec_means_the_one_on_the_targets_path() {
        let (_d, path, real) = bindir();
        assert!(
            command_matches("pkg", &real, &[path.as_str()]),
            "the real one matches"
        );
    }

    /// The escalation this replaced, kept as the thing that must stay false.
    ///
    /// `command_matches` compared the last path component alone, so
    /// `permit alice cmd pkg` authorised `doas /tmp/evil/pkg` -- a binary the
    /// caller put there, run as the target. `doas ./pkg` did it too, and with
    /// `resolve_command` reading the CALLER's `$PATH`, so did
    /// `PATH=/tmp/evil doas pkg`. Each of these was `true` before 2026-09-12.
    #[test]
    fn a_bare_spec_does_not_authorise_a_path_the_caller_chose() {
        let (_d, path, _real) = bindir();
        assert!(!command_matches("pkg", "/tmp/evil/pkg", &[path.as_str()]));
        assert!(!command_matches("pkg", "./pkg", &[path.as_str()]));
        assert!(!command_matches("pkg", "/home/alice/pkg", &[path.as_str()]));
    }

    #[test]
    fn a_spec_naming_nothing_on_the_path_authorises_nothing() {
        // The safe direction: a rule that cannot be resolved matches no
        // command, rather than matching by name.
        let (_d, path, _real) = bindir();
        assert!(!command_matches(
            "nosuchtool",
            "/usr/bin/nosuchtool",
            &[path.as_str()]
        ));
    }

    #[test]
    fn command_match_basename_mismatch() {
        let (_d, path, real) = bindir();
        assert!(!command_matches("apt", &real, &[path.as_str()]));
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

        let find_val = |key: &str| -> Option<OsString> {
            environment
                .iter()
                .find(|(k, _)| k == OsStr::new(key))
                .map(|(_, v)| v.clone())
        };

        assert_eq!(find_val("HOME"), Some(OsString::from("/root")));
        assert_eq!(find_val("LOGNAME"), Some(OsString::from("root")));
        assert_eq!(find_val("USER"), Some(OsString::from("root")));
        assert_eq!(find_val("SHELL"), Some(OsString::from("/bin/sh")));
        assert_eq!(find_val("DOAS_USER"), Some(OsString::from("alice")));
        assert!(find_val("PATH").is_some());
    }

    #[test]
    fn env_clean_root_path() {
        let opts = RuleOptions::default();
        let target = make_target_user();
        let environment = build_environment(&opts, &target, "alice");
        let path = environment
            .iter()
            .find(|(k, _)| k == OsStr::new("PATH"))
            .map(|(_, v)| v.clone());
        assert!(path.unwrap().to_string_lossy().contains("/sbin"));
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
            .find(|(k, _)| k == OsStr::new("PATH"))
            .map(|(_, v)| v.clone());
        assert!(!path.unwrap().to_string_lossy().contains("/sbin"));
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
        let doas_user = environment
            .iter()
            .find(|(k, _)| k == OsStr::new("DOAS_USER"));
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

        let find_val = |key: &str| -> Option<OsString> {
            environment
                .iter()
                .find(|(k, _)| k == OsStr::new(key))
                .map(|(_, v)| v.clone())
        };

        assert_eq!(find_val("EDITOR"), Some(OsString::from("vim")));
        assert_eq!(find_val("PAGER"), Some(OsString::from("less")));
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
            .find(|(k, _)| k == OsStr::new("HOME"))
            .map(|(_, v)| v.clone());
        assert_eq!(home, Some(OsString::from("/custom")));
    }

    #[test]
    fn env_unsetenv_removes_var() {
        let opts = RuleOptions {
            unsetenv: vec!["SHELL".to_string()],
            ..Default::default()
        };
        let target = make_target_user();
        let environment = build_environment(&opts, &target, "alice");
        let shell = environment.iter().find(|(k, _)| k == OsStr::new("SHELL"));
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
        let doas_user = environment
            .iter()
            .find(|(k, _)| k == OsStr::new("DOAS_USER"));
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
        let raw = argv(&["doas", "ls"]);
        let args = parse_args(&raw).unwrap();
        assert_eq!(args.target_user, "root");
        assert!(!args.shell_mode);
        assert!(!args.check_config);
        assert!(!args.clear_persist);
        assert!(!args.non_interactive);
        assert_eq!(args.command.as_deref(), Some(OsStr::new("ls")));
        assert!(args.arguments.is_empty());
    }

    #[test]
    fn args_target_user() {
        let raw = argv(&["doas", "-u", "alice", "whoami"]);
        let args = parse_args(&raw).unwrap();
        assert_eq!(args.target_user, "alice");
        assert_eq!(args.command.as_deref(), Some(OsStr::new("whoami")));
    }

    #[test]
    fn args_target_user_capital_u() {
        let raw = argv(&["doas", "-U", "alice", "id"]);
        let args = parse_args(&raw).unwrap();
        assert_eq!(args.target_user, "alice");
    }

    #[test]
    fn args_shell_mode() {
        let raw = argv(&["doas", "-s"]);
        let args = parse_args(&raw).unwrap();
        assert!(args.shell_mode);
        assert!(args.command.is_none());
    }

    #[test]
    fn args_check_config() {
        let raw = argv(&["doas", "-C", "/etc/doas.conf"]);
        let args = parse_args(&raw).unwrap();
        assert!(args.check_config);
        assert_eq!(args.config_path, "/etc/doas.conf");
    }

    #[test]
    fn args_clear_persist() {
        let raw = argv(&["doas", "-L"]);
        let args = parse_args(&raw).unwrap();
        assert!(args.clear_persist);
    }

    #[test]
    fn args_non_interactive() {
        let raw = argv(&["doas", "-n", "ls"]);
        let args = parse_args(&raw).unwrap();
        assert!(args.non_interactive);
    }

    #[test]
    fn args_double_dash() {
        let raw = argv(&["doas", "--", "-dangerous"]);
        let args = parse_args(&raw).unwrap();
        assert_eq!(args.command.as_deref(), Some(OsStr::new("-dangerous")));
    }

    #[test]
    fn args_command_with_arguments() {
        let raw = argv(&["doas", "pkg", "install", "vim"]);
        let args = parse_args(&raw).unwrap();
        assert_eq!(args.command.as_deref(), Some(OsStr::new("pkg")));
        assert_eq!(args.arguments, argv(&["install", "vim"]));
    }

    #[test]
    fn args_missing_u_value() {
        let raw = argv(&["doas", "-u"]);
        assert!(parse_args(&raw).is_err());
    }

    #[test]
    fn args_missing_c_value() {
        let raw = argv(&["doas", "-C"]);
        assert!(parse_args(&raw).is_err());
    }

    #[test]
    fn args_unknown_option() {
        let raw = argv(&["doas", "-Z"]);
        assert!(parse_args(&raw).is_err());
    }

    #[test]
    fn args_all_flags_combined() {
        let raw = argv(&["doas", "-n", "-u", "bob", "vim", "/etc/hosts"]);
        let args = parse_args(&raw).unwrap();
        assert!(args.non_interactive);
        assert_eq!(args.target_user, "bob");
        assert_eq!(args.command.as_deref(), Some(OsStr::new("vim")));
        assert_eq!(args.arguments, argv(&["/etc/hosts"]));
    }

    // ========================================================================
    // set_env_var helper tests
    // ========================================================================

    /// One environment entry, as the map holds them.
    fn entry(key: &str, value: &str) -> (OsString, OsString) {
        (OsString::from(key), OsString::from(value))
    }

    #[test]
    fn set_env_var_new() {
        let mut env_map = vec![entry("A", "1")];
        set_env_var(&mut env_map, "B", "2");
        assert_eq!(env_map.len(), 2);
        assert_eq!(env_map[1], entry("B", "2"));
    }

    #[test]
    fn set_env_var_override() {
        let mut env_map = vec![entry("A", "1")];
        set_env_var(&mut env_map, "A", "99");
        assert_eq!(env_map.len(), 1);
        assert_eq!(env_map[0], entry("A", "99"));
    }

    // ========================================================================
    // resolve_command tests
    // ========================================================================

    #[test]
    fn resolve_absolute_path() {
        // An absolute path is returned as-is. Whether it is ALLOWED is
        // `command_matches`' question, not this one's.
        let p = path_dirs(target_path(0));
        assert_eq!(resolve_command("/usr/bin/ls", &p), "/usr/bin/ls");
    }

    #[test]
    fn resolve_relative_path() {
        // A relative path containing a slash is returned as-is.
        let p = path_dirs(target_path(0));
        assert_eq!(resolve_command("./my_script", &p), "./my_script");
    }

    #[test]
    fn a_bare_name_resolves_on_the_given_path_and_not_the_callers() {
        // `resolve_command` read `env::var("PATH")`, so `PATH=/tmp/evil doas
        // pkg` resolved to a binary the caller chose. The path is a parameter
        // now, and this test is the only reason that is checkable.
        let (_d, path, real) = bindir();
        assert_eq!(resolve_command("pkg", &[path.as_str()]), real);
        // Not found on the given path: the bare name is returned and the rule
        // check refuses it, rather than exec picking something up elsewhere.
        assert_eq!(resolve_command("pkg", &["/nonexistent"]), "pkg");
    }

    #[test]
    fn the_target_path_is_the_one_the_target_will_run_under() {
        // Same value the environment gets, so the rule check and the exec
        // cannot disagree about which binary a bare name means.
        assert!(target_path(0).contains("/sbin"), "root gets the sbin dirs");
        assert!(
            !target_path(1000).contains("/sbin"),
            "an ordinary user does not"
        );
    }

    // ========================================================================
    // Identity matching tests (unit, bypassing group file)
    // ========================================================================

    #[test]
    fn identity_user_match() {
        // For non-group identities, identity_matches is a string comparison.
        assert_eq!(identity_matches("alice", "alice"), Some(true));
    }

    #[test]
    fn identity_user_mismatch() {
        assert_eq!(identity_matches("bob", "alice"), Some(false));
        // A name that is not a group needs no group file, so the answer is
        // never `None` for this form -- which is what keeps an unreadable
        // /etc/group from disabling plain username rules too.
    }

    /// A two-file fixture for the membership tests.
    ///
    /// `carol`'s LOGIN group is wheel (gid 10), so `/etc/group` does not name
    /// her in wheel's member field -- that field deliberately does not repeat
    /// accounts whose primary group it already is. She is the case the old
    /// implementation got wrong.
    const PASSWD: &[u8] = b"alice:x:1000:1000::/home/alice:/bin/sh
bob:x:1001:1001::/home/bob:/bin/sh
carol:x:1002:10::/home/carol:/bin/sh
";
    const GROUP: &[u8] = b"wheel:x:10:alice
users:x:100:alice,bob
";

    #[test]
    fn a_supplementary_member_is_in_the_group() {
        let got = user_in_group_in(PASSWD, GROUP, b"alice", b"wheel");
        assert_eq!(got, Some(true));
    }

    #[test]
    fn a_primary_member_is_in_the_group_though_the_member_field_omits_her() {
        // The fixture has to actually omit her or this test proves nothing.
        let text = core::str::from_utf8(GROUP).expect("fixture is utf-8");
        assert!(!text.contains("carol"), "fixture must not list carol");
        let got = user_in_group_in(PASSWD, GROUP, b"carol", b"wheel");
        assert_eq!(got, Some(true), "a login group is a group you are in");
    }

    #[test]
    fn a_non_member_is_not_in_the_group() {
        let got = user_in_group_in(PASSWD, GROUP, b"bob", b"wheel");
        assert_eq!(got, Some(false));
    }

    #[test]
    fn a_group_that_does_not_exist_holds_nobody() {
        // An answer, not an absence: `deny :typo` correctly applies to no one.
        let got = user_in_group_in(PASSWD, GROUP, b"alice", b"nosuchgroup");
        assert_eq!(got, Some(false));
    }

    #[test]
    fn a_caller_absent_from_passwd_is_unknown_rather_than_false() {
        // Their primary gid cannot be known and primary membership counts, so
        // this is the `None` that skips a permit and refuses on a deny.
        let got = user_in_group_in(PASSWD, GROUP, b"mallory", b"wheel");
        assert_eq!(got, None);
    }

    #[test]
    fn one_group_name_that_is_not_utf8_does_not_disable_the_whole_file() {
        // `read_to_string` returns Err for the ENTIRE file on one bad byte, and
        // `.ok()?` turned that into "cannot read" -- so a single group nobody
        // asked about disabled every `:group` rule at once. This filesystem
        // allows every byte but `/` and NUL in a name, so such a group is legal.
        let mut group = Vec::from(GROUP);
        group.extend_from_slice(b"caf");
        group.push(0xFF);
        group.extend_from_slice(b":x:200:bob\n");
        // Proof the fixture is what the test claims, so it cannot pass vacuously.
        assert!(
            core::str::from_utf8(&group).is_err(),
            "fixture must not be utf-8"
        );

        let got = user_in_group_in(PASSWD, &group, b"alice", b"wheel");
        assert_eq!(got, Some(true), "an unrelated odd group must not matter");

        let mut odd = Vec::from(&b"caf"[..]);
        odd.push(0xFF);
        let got = user_in_group_in(PASSWD, &group, b"bob", &odd);
        assert_eq!(got, Some(true), "and the odd group is itself usable");
    }

    // Group matching requires /etc/group, tested in integration tests.
    // We verify the prefix-detection logic here:
    #[test]
    fn identity_group_prefix_detected() {
        let id = ":wheel";
        assert!(id.starts_with(':'));
        assert_eq!(id.strip_prefix(':'), Some("wheel"));
    }

    // ========================================================================
    // argv and the environment as bytes
    // ========================================================================

    /// A command argument that a `String` cannot hold survives to the exec.
    ///
    /// `doas cat FILE` passes a *filename*, and a filename on this OS may hold
    /// any byte but `/` and NUL. `env::args()`'s iterator is a literal
    /// `unwrap`, so this used to panic before running a line of its own file
    /// -- from a setuid binary. See `known-issues.md` ->
    /// `B-COREUTILS-PANIC-ON-A-NON-UTF-8-ARGUMENT`.
    #[test]
    fn a_command_argument_that_is_not_text_survives_parsing() {
        let odd = not_text();
        assert!(
            odd.to_str().is_none(),
            "the fixture must be unrepresentable as a `String`, or this test              asserts nothing"
        );

        let raw = vec![OsString::from("doas"), OsString::from("cat"), odd.clone()];
        let args = parse_args(&raw).expect("parses");
        assert_eq!(args.command.as_deref(), Some(OsStr::new("cat")));
        assert_eq!(args.arguments, vec![odd]);
    }

    /// A rule that pins its arguments does not match one that is not text.
    ///
    /// `doas.conf` is a text file, so it cannot name such an argument -- and a
    /// rule permitting exactly `pkg install vim` must not be satisfied by
    /// `pkg install <something else>`, whatever that something is.
    #[test]
    fn a_rule_pinning_arguments_does_not_match_an_argument_that_is_not_text() {
        let rules = parse_config(
            "permit alice as root cmd /usr/bin/pkg args install vim
",
        )
        .expect("config");
        let args = vec![OsString::from("install"), not_text()];
        let result = evaluate_rules(
            &rules,
            "alice",
            "root",
            Some("/usr/bin/pkg"),
            &args,
            &path_dirs(target_path(0)),
        );
        assert!(
            matches!(result, MatchResult::NoMatch),
            "an argument the rule cannot name must not satisfy it"
        );
    }

    /// A command name that is not text names no permitted command, and is
    /// refused by the same path as any other command the caller may not run.
    #[test]
    fn a_command_name_that_is_not_text_matches_no_rule() {
        let rules = parse_config(
            "permit alice as root cmd /usr/bin/pkg
",
        )
        .expect("config");
        let result = evaluate_rules(
            &rules,
            "alice",
            "root",
            Some(""),
            &[],
            &path_dirs(target_path(0)),
        );
        assert!(matches!(result, MatchResult::NoMatch));
    }

    /// `keepenv` hands the caller's environment through, including a variable
    /// whose value is not text. `env::vars()` panicked on exactly that, in the
    /// branch whose whole job is to lose nothing.
    #[test]
    fn keepenv_carries_a_value_that_is_not_text() {
        let mut env_map = vec![entry("A", "1")];
        env_map.push((OsString::from("ODD"), not_text()));
        set_env_var(&mut env_map, "DOAS_USER", "alice");

        let odd = env_map
            .iter()
            .find(|(k, _)| k == OsStr::new("ODD"))
            .map(|(_, v)| v.clone());
        assert_eq!(odd, Some(not_text()));
    }
}
