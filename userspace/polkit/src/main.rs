//! Slate OS PolicyKit Authorization Framework
//!
//! A multi-personality binary providing:
//! - **polkitd** -- PolicyKit authorization daemon that manages policy rules,
//!   loads `.policy` XML action definitions, and answers authorization queries.
//! - **pkexec** -- Execute a command as another user after PolicyKit authorization.
//! - **pkaction** -- List and inspect registered PolicyKit actions.
//! - **pkcheck** -- Check whether a process is authorized for a given action.
//!
//! # Architecture
//!
//! PolicyKit separates *who may do what* from the programs that need privileges.
//! Actions are defined in `.policy` XML files installed under `/usr/share/polkit-1/actions/`.
//! Authorization rules (JavaScript-like on real polkit, simplified here to a
//! declarative YAML format) live in `/etc/polkit-1/rules.d/` and
//! `/usr/share/polkit-1/rules.d/`.
//!
//! Authorization results are one of:
//! - `yes` -- unconditionally allowed
//! - `no` -- unconditionally denied
//! - `auth_admin` -- allowed after an administrator authenticates
//! - `auth_self` -- allowed after the requesting user authenticates
//!
//! # Personality detection
//!
//! The binary inspects `argv[0]` to decide which personality to run.
//! It also accepts a subcommand (`polkit daemon`, `polkit exec`, etc.)
//! as a fallback.

#![cfg_attr(not(test), no_main)]
#![cfg_attr(test, allow(dead_code))]

use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::{self, Write as IoWrite};

use userdb::{Auth, Record, UserDb};

// ============================================================================
// Authorization result
// ============================================================================

/// The possible outcomes of an authorization check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AuthResult {
    /// Unconditionally allowed.
    Yes,
    /// Unconditionally denied.
    No,
    /// Allowed after an administrator authenticates.
    AuthAdmin,
    /// Allowed after the requesting user authenticates.
    AuthSelf,
}

impl AuthResult {
    fn as_str(self) -> &'static str {
        match self {
            Self::Yes => "yes",
            Self::No => "no",
            Self::AuthAdmin => "auth_admin",
            Self::AuthSelf => "auth_self",
        }
    }

    fn from_str(s: &str) -> Option<Self> {
        match s.trim() {
            "yes" => Some(Self::Yes),
            "no" => Some(Self::No),
            "auth_admin" => Some(Self::AuthAdmin),
            "auth_self" => Some(Self::AuthSelf),
            _ => None,
        }
    }
}

impl core::fmt::Display for AuthResult {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

// ============================================================================
// Action definition (from .policy XML files)
// ============================================================================

/// A registered PolicyKit action, parsed from a `.policy` XML file.
#[derive(Debug, Clone)]
struct Action {
    /// Unique action identifier, e.g. `org.slateos.pkexec.run-program`.
    id: String,
    /// Short human-readable description.
    description: String,
    /// Longer help message.
    message: String,
    /// Icon name (optional).
    icon_name: String,
    /// Default authorization for inactive sessions.
    defaults_inactive: AuthResult,
    /// Default authorization for active sessions.
    defaults_active: AuthResult,
    /// Default authorization for any session.
    defaults_any: AuthResult,
    /// Vendor name.
    vendor: String,
    /// Vendor URL.
    vendor_url: String,
    /// Annotations (key=value pairs).
    annotations: HashMap<String, String>,
}

impl Action {
    fn new(id: &str) -> Self {
        Self {
            id: id.to_string(),
            description: String::new(),
            message: String::new(),
            icon_name: String::new(),
            defaults_inactive: AuthResult::No,
            defaults_active: AuthResult::AuthAdmin,
            defaults_any: AuthResult::No,
            vendor: String::new(),
            vendor_url: String::new(),
            annotations: HashMap::new(),
        }
    }
}

// ============================================================================
// Authorization rule
// ============================================================================

/// A declarative authorization rule loaded from a rules file.
///
/// Rules are matched in order. The first matching rule wins.
#[derive(Debug, Clone)]
struct Rule {
    /// Action ID pattern. Supports trailing wildcards: `org.slateos.*` matches
    /// any action starting with `org.slateos.`.
    action_pattern: String,
    /// If set, the rule only applies to this user.
    user: Option<String>,
    /// If set, the rule only applies to members of this group.
    group: Option<String>,
    /// The authorization result to return when the rule matches.
    result: AuthResult,
    /// Priority (lower = evaluated first within the same file).
    priority: i32,
}

impl Rule {
    /// Check whether `action_id` matches this rule's pattern.
    fn matches_action(&self, action_id: &str) -> bool {
        action_pattern_matches(&self.action_pattern, action_id)
    }
}

/// Test whether an action pattern matches a given action ID.
///
/// Supports:
/// - Exact match: `org.slateos.foo` matches `org.slateos.foo`
/// - Trailing wildcard: `org.slateos.*` matches `org.slateos.foo` and `org.slateos.bar.baz`
/// - Universal wildcard: `*` matches everything
fn action_pattern_matches(pattern: &str, action_id: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if let Some(prefix) = pattern.strip_suffix(".*") {
        // Wildcard: the action must start with the prefix followed by a dot.
        action_id == prefix || action_id.starts_with(&format!("{prefix}."))
    } else if let Some(prefix) = pattern.strip_suffix('*') {
        action_id.starts_with(prefix)
    } else {
        pattern == action_id
    }
}

// ============================================================================
// User information (lightweight, shared with su/useradm)
// ============================================================================

/// Read the user database, treating an absent or unreadable file as empty.
///
/// Empty is the safe answer *here* because every caller of this function uses
/// the result to grant something: an unreadable database authorises nobody.
/// The same shape of fallback in `sudo` was an authentication bypass, because
/// there the empty case meant "no policy stands in the way" rather than "no
/// user qualifies" — which is why this comment exists rather than the idiom
/// being copied around.
fn read_users() -> UserDb {
    match UserDb::load(userdb::DEFAULT_PATH) {
        Ok(db) => db,
        Err(e) => {
            if e.kind() != io::ErrorKind::NotFound {
                eprintln!("polkit: cannot read {}: {e}", userdb::DEFAULT_PATH);
            }
            UserDb::new()
        }
    }
}

/// The record's login name, or the empty string if it has none.
///
/// A record with no `username` can match no rule and be authenticated as
/// nobody; the empty string makes it fall out of the ordinary comparisons
/// without a special case at each one.
fn name_of(record: &Record) -> String {
    record.username().unwrap_or_default()
}

/// The record's uid, or `u32::MAX` if it has none.
///
/// `u32::MAX` is the same value `get_caller_uid` reports for an unidentifiable
/// caller, and no account is ever created with it, so a record missing its uid
/// matches no lookup rather than colliding with root at 0.
fn uid_of(record: &Record) -> u32 {
    record.uid().unwrap_or(u32::MAX)
}

/// Get the current user's UID from `/proc/self/status` or the `USER` env var.
fn get_caller_uid(users: &UserDb) -> u32 {
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

    if let Ok(name) = env::var("USER")
        && let Some(user) = users.find(&name)
    {
        return uid_of(user);
    }

    u32::MAX
}

// ============================================================================
// Authentication
// ============================================================================
//
// There is deliberately no hash function in this crate. It used to carry a
// SHA-256 and a `hash_password(password, salt) = sha256(salt + password)`,
// tested against three known-answer SHA-256 vectors — which proved the SHA-256
// was SHA-256, and proved nothing whatever about whether the *composition*
// matched what `useradm` writes. It did not. Every password check here failed
// against every real database. `Record::check_password` recomputes the stored
// crypt(3) setting instead, which cannot disagree with the writer because the
// writer uses the same call.

/// Prompt an admin user to authenticate, returning whether they did.
///
/// The database's list of administrators is shown before the prompt, which is
/// how the real polkit agent behaves: the caller has to know *which* accounts
/// can approve the action in order to go and find someone holding one.
fn authenticate_admin(users: &UserDb) -> bool {
    let admins: Vec<&Record> = users.records().iter().filter(|u| u.is_admin()).collect();
    if admins.is_empty() {
        eprintln!("polkit: no admin users configured");
        return false;
    }

    let _ = writeln!(
        io::stderr(),
        "Authentication required. Admin users: {}",
        admins
            .iter()
            .map(|u| name_of(u))
            .collect::<Vec<_>>()
            .join(", ")
    );

    eprint!("Username: ");
    let _ = io::stderr().flush();
    let mut username = String::new();
    if io::stdin().read_line(&mut username).is_err() {
        eprintln!("polkit: failed to read username");
        return false;
    }
    let username = username.trim();

    let Some(admin) = admins.iter().find(|u| name_of(u) == username) else {
        eprintln!("polkit: user '{username}' is not an admin");
        return false;
    };

    prompt_and_check(admin)
}

/// Prompt the calling user to authenticate themselves.
fn authenticate_self(user: &Record) -> bool {
    let _ = writeln!(
        io::stderr(),
        "Authentication required for user '{}'.",
        name_of(user)
    );

    prompt_and_check(user)
}

/// Read a password from stdin and check it against `record`.
///
/// The five outcomes are spelled out rather than collapsed to a bool because
/// three of them are administrator errors, not wrong passwords, and a user who
/// is told "authentication failure" when the real answer is "that account was
/// never given a password" has no way to act on it.
fn prompt_and_check(record: &Record) -> bool {
    if record.is_locked() {
        eprintln!("polkit: account '{}' is locked", name_of(record));
        return false;
    }
    if record.has_legacy_password() {
        eprintln!(
            "polkit: account '{}' has a password stored in a format that predates \
             this system's hashing; run `useradm passwd {}` to reset it",
            name_of(record),
            name_of(record)
        );
        return false;
    }

    eprint!("Password: ");
    let _ = io::stderr().flush();
    let mut password = String::new();
    if io::stdin().read_line(&mut password).is_err() {
        eprintln!("polkit: failed to read password");
        return false;
    }
    let password = password.trim();

    match record.check_password(password) {
        Auth::Accepted => true,
        Auth::NoPassword => {
            eprintln!(
                "polkit: account '{}' has no password set and cannot authenticate",
                name_of(record)
            );
            false
        }
        Auth::Rejected | Auth::Locked | Auth::Unusable => false,
    }
}

// ============================================================================
// .policy XML parser (minimal, handles the standard polkit schema)
// ============================================================================

/// Directory containing `.policy` XML files.
const POLICY_DIR: &str = "/usr/share/polkit-1/actions";

/// Parse a `.policy` XML file and return the actions defined in it.
///
/// This is a minimal XML parser sufficient for the polkit `.policy` format:
/// ```xml
/// <?xml version="1.0" encoding="UTF-8"?>
/// <policyconfig>
///   <vendor>Slate OS</vendor>
///   <vendor_url>https://slateos.example.com</vendor_url>
///   <action id="org.slateos.example">
///     <description>Do something</description>
///     <message>Authentication is required to do something</message>
///     <icon_name>dialog-password</icon_name>
///     <defaults>
///       <allow_any>no</allow_any>
///       <allow_inactive>no</allow_inactive>
///       <allow_active>auth_admin</allow_active>
///     </defaults>
///     <annotate key="org.slateos.policykit.exec.path">/usr/bin/something</annotate>
///   </action>
/// </policyconfig>
/// ```
fn parse_policy_xml(content: &str) -> Vec<Action> {
    let mut actions = Vec::new();
    let mut vendor = String::new();
    let mut vendor_url = String::new();

    // Track state: are we inside an <action>, <defaults>, etc.
    let mut in_action = false;
    let mut in_defaults = false;
    let mut current_action: Option<Action> = None;

    for line in content.lines() {
        let trimmed = line.trim();

        // Top-level vendor info.
        if !in_action {
            if let Some(val) = extract_xml_text(trimmed, "vendor") {
                vendor = val;
                continue;
            }
            if let Some(val) = extract_xml_text(trimmed, "vendor_url") {
                vendor_url = val;
                continue;
            }
        }

        // <action id="...">
        if let Some(id) = extract_action_id(trimmed) {
            let mut action = Action::new(&id);
            action.vendor = vendor.clone();
            action.vendor_url = vendor_url.clone();
            current_action = Some(action);
            in_action = true;
            in_defaults = false;
            continue;
        }

        if trimmed == "</action>" {
            if let Some(action) = current_action.take() {
                actions.push(action);
            }
            in_action = false;
            in_defaults = false;
            continue;
        }

        if !in_action {
            continue;
        }

        // Inside <action>: check for <defaults>
        if trimmed == "<defaults>" {
            in_defaults = true;
            continue;
        }
        if trimmed == "</defaults>" {
            in_defaults = false;
            continue;
        }

        if in_defaults {
            if let Some(ref mut action) = current_action {
                if let Some(val) = extract_xml_text(trimmed, "allow_any") {
                    if let Some(r) = AuthResult::from_str(&val) {
                        action.defaults_any = r;
                    }
                } else if let Some(val) = extract_xml_text(trimmed, "allow_inactive") {
                    if let Some(r) = AuthResult::from_str(&val) {
                        action.defaults_inactive = r;
                    }
                } else if let Some(val) = extract_xml_text(trimmed, "allow_active")
                    && let Some(r) = AuthResult::from_str(&val)
                {
                    action.defaults_active = r;
                }
            }
        } else if let Some(ref mut action) = current_action {
            if let Some(val) = extract_xml_text(trimmed, "description") {
                action.description = val;
            } else if let Some(val) = extract_xml_text(trimmed, "message") {
                action.message = val;
            } else if let Some(val) = extract_xml_text(trimmed, "icon_name") {
                action.icon_name = val;
            } else if let Some((key, val)) = extract_annotate(trimmed) {
                action.annotations.insert(key, val);
            }
        }
    }

    // Handle unclosed <action> (malformed, but be lenient).
    if let Some(action) = current_action {
        actions.push(action);
    }

    actions
}

/// Extract text content from a simple XML element: `<tag>text</tag>`.
fn extract_xml_text(line: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    if let Some(rest) = line.strip_prefix(&open)
        && let Some(text) = rest.strip_suffix(&close)
    {
        return Some(text.to_string());
    }
    None
}

/// Extract action id from `<action id="...">`.
fn extract_action_id(line: &str) -> Option<String> {
    let prefix = "<action id=\"";
    if let Some(rest) = line.strip_prefix(prefix)
        && let Some((id, _after)) = rest.split_once('"')
    {
        return Some(id.to_string());
    }
    None
}

/// Extract an annotation: `<annotate key="key">value</annotate>`.
fn extract_annotate(line: &str) -> Option<(String, String)> {
    // `split_once` rather than `find` plus two slices: it yields both halves
    // already past the delimiter, so there is no `end_quote + 1` whose
    // correctness rests on the delimiter being one byte wide.
    let prefix = "<annotate key=\"";
    if let Some(rest) = line.strip_prefix(prefix)
        && let Some((key, after_key)) = rest.split_once('"')
        // Skip the `>`
        && let Some(after_gt) = after_key.strip_prefix('>')
        && let Some((val, _tail)) = after_gt.split_once("</annotate>")
    {
        return Some((key.to_string(), val.to_string()));
    }
    None
}

// ============================================================================
// Rules parser (YAML-like declarative format)
// ============================================================================

/// Directories containing authorization rules.
const RULES_DIRS: &[&str] = &["/etc/polkit-1/rules.d", "/usr/share/polkit-1/rules.d"];

/// Parse a rules file.
///
/// Format (one rule per YAML document-like block):
/// ```yaml
/// - action: org.slateos.pkexec.*
///   user: alice
///   result: yes
///   priority: 10
///
/// - action: org.slateos.mount.*
///   group: storage
///   result: auth_self
///   priority: 50
/// ```
fn parse_rules_file(content: &str) -> Vec<Rule> {
    let mut rules = Vec::new();
    let mut current: Option<Rule> = None;

    for line in content.lines() {
        let trimmed = line.trim();

        // Skip comments and blank lines.
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        // New rule starts with `- action:`.
        if let Some(rest) = trimmed.strip_prefix("- action:") {
            if let Some(rule) = current.take() {
                rules.push(rule);
            }
            current = Some(Rule {
                action_pattern: rest.trim().trim_matches('"').to_string(),
                user: None,
                group: None,
                result: AuthResult::No,
                priority: 100,
            });
            continue;
        }

        if let Some(ref mut rule) = current {
            if let Some(val) = trimmed.strip_prefix("user:") {
                rule.user = Some(val.trim().trim_matches('"').to_string());
            } else if let Some(val) = trimmed.strip_prefix("group:") {
                rule.group = Some(val.trim().trim_matches('"').to_string());
            } else if let Some(val) = trimmed.strip_prefix("result:") {
                if let Some(r) = AuthResult::from_str(val.trim().trim_matches('"')) {
                    rule.result = r;
                }
            } else if let Some(val) = trimmed.strip_prefix("priority:")
                && let Ok(p) = val.trim().parse::<i32>()
            {
                rule.priority = p;
            }
        }
    }

    if let Some(rule) = current {
        rules.push(rule);
    }

    rules
}

// ============================================================================
// Policy store: loads actions and rules
// ============================================================================

/// All loaded policy data.
struct PolicyStore {
    actions: Vec<Action>,
    rules: Vec<Rule>,
}

impl PolicyStore {
    /// Load all `.policy` files and rules from the standard directories.
    fn load() -> Self {
        let actions = Self::load_actions();
        let mut rules = Self::load_rules();
        // Sort rules by priority (lower priority number = evaluated first).
        rules.sort_by_key(|r| r.priority);
        Self { actions, rules }
    }

    /// Load all `.policy` XML files from the actions directory.
    fn load_actions() -> Vec<Action> {
        let mut actions = Vec::new();
        if let Ok(entries) = fs::read_dir(POLICY_DIR) {
            for entry in entries {
                let Ok(entry) = entry else { continue };
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("policy")
                    && let Ok(content) = fs::read_to_string(&path)
                {
                    actions.extend(parse_policy_xml(&content));
                }
            }
        }
        actions
    }

    /// Load all rules files from the rules directories.
    fn load_rules() -> Vec<Rule> {
        let mut rules = Vec::new();
        for dir in RULES_DIRS {
            if let Ok(entries) = fs::read_dir(dir) {
                let mut files: Vec<_> = entries
                    .filter_map(|e| e.ok())
                    .map(|e| e.path())
                    .filter(|p| {
                        p.extension().and_then(|e| e.to_str()) == Some("rules")
                            || p.extension().and_then(|e| e.to_str()) == Some("yaml")
                    })
                    .collect();
                // Sort by filename for deterministic evaluation order.
                files.sort();
                for path in &files {
                    if let Ok(content) = fs::read_to_string(path) {
                        rules.extend(parse_rules_file(&content));
                    }
                }
            }
        }
        rules
    }

    /// Find an action by its ID.
    fn find_action(&self, action_id: &str) -> Option<&Action> {
        self.actions.iter().find(|a| a.id == action_id)
    }

    /// Check authorization for a user performing an action.
    ///
    /// Evaluation order:
    /// 1. Check explicit rules (sorted by priority) for a match.
    /// 2. Fall back to the action's defaults for active sessions.
    /// 3. If the action is unknown, deny.
    fn check_authorization(
        &self,
        action_id: &str,
        user: &Record,
        is_active_session: bool,
    ) -> AuthResult {
        let username = name_of(user);
        // An administrator is a member of `wheel` for the purpose of matching a
        // group rule. The database records administrator-ness as a flag rather
        // than as group membership, so a rule written `group: wheel` — the
        // spelling every polkit example uses — would otherwise match nobody on
        // a machine whose admins were made by `useradm`.
        let mut groups = user.groups();
        if user.is_admin() && !groups.iter().any(|g| g == "wheel") {
            groups.push("wheel".to_string());
        }

        // 1. Explicit rules.
        for rule in &self.rules {
            if !rule.matches_action(action_id) {
                continue;
            }

            // Check user constraint.
            if let Some(ref rule_user) = rule.user
                && *rule_user != username
            {
                continue;
            }

            // Check group constraint.
            if let Some(ref rule_group) = rule.group
                && !groups.iter().any(|g| g == rule_group)
            {
                continue;
            }

            return rule.result;
        }

        // 2. Action defaults.
        if let Some(action) = self.find_action(action_id) {
            return if is_active_session {
                action.defaults_active
            } else {
                action.defaults_inactive
            };
        }

        // 3. Unknown action: deny.
        AuthResult::No
    }
}

// ============================================================================
// Personality: polkitd (daemon)
// ============================================================================

/// Run the polkitd daemon personality.
///
/// In a full implementation, this would listen on a D-Bus well-known name
/// and answer authorization queries from other processes. For now, it loads
/// the policy store, reports what it found, and enters a simple command loop
/// on stdin for testing.
fn run_polkitd(args: &[String]) -> i32 {
    let mut foreground = false;
    let mut replace = false;
    let mut no_debug = false;

    // A slice cursor rather than an index, here and in the three other option
    // loops below. `split_first` is what makes an option-with-a-value safe: the
    // value is taken from the tail that is known to exist, so there is no
    // `i + 1` to bounds-check separately from the `args[i + 1]` that follows it.
    let mut rest = args;
    while let Some((arg, tail)) = rest.split_first() {
        rest = tail;
        match arg.as_str() {
            "--no-debug" => no_debug = true,
            "--replace" | "-r" => replace = true,
            "--help" | "-h" => {
                print_polkitd_usage();
                return 0;
            }
            "--version" | "-V" => {
                println!("polkitd 0.1.0 (Slate OS)");
                return 0;
            }
            _ => {
                if arg.starts_with('-') {
                    eprintln!("polkitd: unknown option: {arg}");
                    return 1;
                }
                foreground = true; // positional: treat as foreground flag
            }
        }
    }

    // In a daemon, we would fork into the background. On Slate OS the service
    // manager starts us, so we always run in the foreground.
    let _ = foreground;
    let _ = replace;

    let store = PolicyStore::load();

    if !no_debug {
        let _ = writeln!(
            io::stderr(),
            "polkitd: loaded {} actions, {} rules",
            store.actions.len(),
            store.rules.len()
        );
        for action in &store.actions {
            let _ = writeln!(
                io::stderr(),
                "  action: {} (active={}, inactive={}, any={})",
                action.id,
                action.defaults_active,
                action.defaults_inactive,
                action.defaults_any,
            );
        }
    }

    // Write PID file for the service manager.
    let pid = std::process::id();
    let _ = fs::create_dir_all("/run/polkit-1");
    let _ = fs::write("/run/polkit-1/polkitd.pid", format!("{pid}\n"));

    println!("polkitd: ready (pid {pid})");

    // Simple interactive command loop (for testing / non-D-Bus mode).
    // Commands:
    //   CHECK <action_id> <uid>    -- check authorization
    //   LIST                       -- list loaded actions
    //   RELOAD                     -- reload policy store
    //   QUIT                       -- exit
    let users = read_users();
    let mut store = store;
    let mut line = String::new();
    loop {
        line.clear();
        match io::stdin().read_line(&mut line) {
            Ok(0) | Err(_) => break, // EOF or error
            Ok(_) => {}
        }
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.is_empty() {
            continue;
        }

        let Some((command, operands)) = parts.split_first() else {
            continue;
        };

        match command.to_uppercase().as_str() {
            "CHECK" => {
                // A slice pattern rather than a length test followed by two
                // indexes: the guard and the accesses were two statements of
                // one fact, and only the pattern keeps them from disagreeing.
                let [action_id, uid_text, ..] = operands else {
                    println!("ERR: usage: CHECK <action_id> <uid>");
                    continue;
                };
                let uid: u32 = match uid_text.parse() {
                    Ok(u) => u,
                    Err(_) => {
                        println!("ERR: invalid uid");
                        continue;
                    }
                };
                let user = match users.find_uid(uid) {
                    Some(u) => u,
                    None => {
                        println!("ERR: unknown uid {uid}");
                        continue;
                    }
                };
                let result = store.check_authorization(action_id, user, true);
                println!("OK: {result}");
            }
            "LIST" => {
                for action in &store.actions {
                    println!("{}", action.id);
                }
                println!("OK: {} actions", store.actions.len());
            }
            "RELOAD" => {
                store = PolicyStore::load();
                println!(
                    "OK: reloaded {} actions, {} rules",
                    store.actions.len(),
                    store.rules.len()
                );
            }
            "QUIT" | "EXIT" => {
                break;
            }
            _ => {
                println!("ERR: unknown command '{command}'");
            }
        }
        let _ = io::stdout().flush();
    }

    // Clean up PID file.
    let _ = fs::remove_file("/run/polkit-1/polkitd.pid");
    0
}

fn print_polkitd_usage() {
    println!("Usage: polkitd [OPTIONS]");
    println!();
    println!("Slate OS PolicyKit authorization daemon.");
    println!();
    println!("Options:");
    println!("  --no-debug     Suppress debug output on stderr");
    println!("  --replace, -r  Replace a running instance");
    println!("  --version, -V  Print version and exit");
    println!("  --help, -h     Print this help and exit");
}

// ============================================================================
// Personality: pkexec (execute as another user)
// ============================================================================

/// Allowlist of environment variables that pkexec preserves.
const SAFE_ENV_VARS: &[&str] = &[
    "TERM",
    "COLORTERM",
    "DISPLAY",
    "XAUTHORITY",
    "WAYLAND_DISPLAY",
    "LANG",
    "LANGUAGE",
    "LC_ALL",
    "LC_CTYPE",
    "LC_MESSAGES",
    "HOME",
    "USER",
    "LOGNAME",
    "SHELL",
    "PATH",
];

/// Run the pkexec personality: execute a command as another user.
fn run_pkexec(args: &[String]) -> i32 {
    let mut target_user = "root".to_string();
    let mut allow_gui = false;
    let mut disable_internal = false;
    let mut command_args: Vec<String> = Vec::new();
    let mut found_command = false;

    let mut rest = args;
    while let Some((arg, tail)) = rest.split_first() {
        rest = tail;
        if found_command {
            command_args.push(arg.clone());
            continue;
        }
        match arg.as_str() {
            "--help" | "-h" => {
                print_pkexec_usage();
                return 0;
            }
            "--version" | "-V" => {
                println!("pkexec 0.1.0 (Slate OS)");
                return 0;
            }
            "--user" => {
                let Some((value, after_value)) = rest.split_first() else {
                    eprintln!("pkexec: --user requires a username");
                    return 127;
                };
                target_user = value.clone();
                rest = after_value;
            }
            "--disable-internal-agent" => {
                disable_internal = true;
            }
            "--keep-cwd" | "--allow-gui" => {
                allow_gui = true;
            }
            _ => {
                if !arg.starts_with('-') {
                    // First non-option argument is the command.
                    command_args.push(arg.clone());
                    found_command = true;
                } else if let Some(val) = arg.strip_prefix("--user=") {
                    target_user = val.to_string();
                } else {
                    eprintln!("pkexec: unknown option: {arg}");
                    return 127;
                }
            }
        }
    }

    // Binds the command path in the same step that proves there is one, so the
    // two uses below cannot outlive the emptiness check the way an index would.
    let Some(command_path) = command_args.first().cloned() else {
        eprintln!("pkexec: no command specified");
        print_pkexec_usage();
        return 127;
    };

    let _ = allow_gui;
    let _ = disable_internal;

    let users = read_users();
    let caller_uid = get_caller_uid(&users);
    let caller = users.find_uid(caller_uid).cloned();

    // Root can do anything without authentication.
    if caller_uid == 0 {
        return exec_command(&command_args, &target_user, &users);
    }

    let caller = match caller {
        Some(c) => c,
        None => {
            eprintln!("pkexec: cannot identify calling user (uid {caller_uid})");
            return 127;
        }
    };

    // Determine the action ID. If the command has a polkit annotation, use it;
    // otherwise use the generic pkexec action.
    let action_id = determine_pkexec_action(&command_path);

    let store = PolicyStore::load();
    let result = store.check_authorization(&action_id, &caller, true);

    match result {
        AuthResult::Yes => exec_command(&command_args, &target_user, &users),
        AuthResult::No => {
            eprintln!("pkexec: not authorized to execute '{command_path}' as '{target_user}'");
            126
        }
        AuthResult::AuthAdmin => {
            if !disable_internal && authenticate_admin(&users) {
                exec_command(&command_args, &target_user, &users)
            } else {
                eprintln!("pkexec: authentication failed");
                126
            }
        }
        AuthResult::AuthSelf => {
            if !disable_internal && authenticate_self(&caller) {
                exec_command(&command_args, &target_user, &users)
            } else {
                eprintln!("pkexec: authentication failed");
                126
            }
        }
    }
}

/// Determine the PolicyKit action ID for a pkexec invocation.
///
/// Looks for a matching annotation `org.slateos.policykit.exec.path` in the
/// loaded actions. Falls back to `org.slateos.policykit.exec`.
fn determine_pkexec_action(command_path: &str) -> String {
    let store = PolicyStore::load();
    for action in &store.actions {
        if let Some(path) = action.annotations.get("org.slateos.policykit.exec.path")
            && path == command_path
        {
            return action.id.clone();
        }
    }
    "org.slateos.policykit.exec".to_string()
}

/// Execute a command as the target user with a sanitized environment.
fn exec_command(command_args: &[String], target_user: &str, users: &UserDb) -> i32 {
    let target = users.find(target_user);

    // Sanitize environment: only keep safe variables.
    let saved: Vec<(String, String)> = SAFE_ENV_VARS
        .iter()
        .filter_map(|key| env::var(key).ok().map(|val| (key.to_string(), val)))
        .collect();

    // On Slate OS we would use exec() to replace the process. Since we cannot
    // exec in this stub environment, we simulate with std::process::Command.
    //
    // `split_first` rather than `[0]` and `[1..]`: every caller checks the slice
    // is non-empty first, but a function that runs a command as another user is
    // the wrong place to trust a caller's guard. 127 is the shell's "command not
    // found", which is what an empty argv amounts to.
    let Some((program, program_args)) = command_args.split_first() else {
        eprintln!("pkexec: no command to execute");
        return 127;
    };

    let mut cmd = std::process::Command::new(program);
    cmd.args(program_args);

    // Clear environment and set only safe vars + target identity.
    cmd.env_clear();
    for (key, val) in &saved {
        cmd.env(key, val);
    }

    // Override identity variables for the target user.
    if let Some(t) = target {
        let name = name_of(t);
        cmd.env("USER", &name);
        cmd.env("LOGNAME", &name);
        // The database's own `home_dir` rather than `/home/<name>`: an account
        // whose home was moved — root's `/root` above all — would otherwise be
        // handed a directory that does not exist, and every program the command
        // runs that writes a dotfile would fail in a different place.
        cmd.env("HOME", t.home().unwrap_or_else(|| format!("/home/{name}")));
    } else {
        cmd.env("USER", target_user);
        cmd.env("LOGNAME", target_user);
    }
    cmd.env("PKEXEC_UID", caller_uid_string());

    match cmd.status() {
        Ok(status) => status.code().unwrap_or(1),
        Err(e) => {
            eprintln!("pkexec: failed to execute '{program}': {e}");
            127
        }
    }
}

/// Return the caller's UID as a string for the PKEXEC_UID env var.
fn caller_uid_string() -> String {
    if let Ok(content) = fs::read_to_string("/proc/self/status") {
        for line in content.lines() {
            if let Some(rest) = line.strip_prefix("Uid:")
                && let Some(uid_str) = rest.split_whitespace().next()
            {
                return uid_str.to_string();
            }
        }
    }
    env::var("UID").unwrap_or_else(|_| "0".to_string())
}

fn print_pkexec_usage() {
    println!("Usage: pkexec [OPTIONS] COMMAND [ARGS...]");
    println!();
    println!("Execute COMMAND as another user, authorized via PolicyKit.");
    println!();
    println!("Options:");
    println!("  --user USER                Run as USER (default: root)");
    println!("  --disable-internal-agent   Do not use built-in auth agent");
    println!("  --keep-cwd                 Keep current working directory");
    println!("  --version, -V              Print version and exit");
    println!("  --help, -h                 Print this help and exit");
}

// ============================================================================
// Personality: pkaction (list registered actions)
// ============================================================================

/// Run the pkaction personality: list or inspect registered actions.
fn run_pkaction(args: &[String]) -> i32 {
    let mut verbose = false;
    let mut action_id_filter: Option<String> = None;

    let mut rest = args;
    while let Some((arg, tail)) = rest.split_first() {
        rest = tail;
        match arg.as_str() {
            "--verbose" | "-v" => verbose = true,
            "--help" | "-h" => {
                print_pkaction_usage();
                return 0;
            }
            "--version" | "-V" => {
                println!("pkaction 0.1.0 (Slate OS)");
                return 0;
            }
            "--action-id" => {
                let Some((value, after_value)) = rest.split_first() else {
                    eprintln!("pkaction: --action-id requires a value");
                    return 1;
                };
                action_id_filter = Some(value.clone());
                rest = after_value;
            }
            _ => {
                if let Some(val) = arg.strip_prefix("--action-id=") {
                    action_id_filter = Some(val.to_string());
                } else if arg.starts_with('-') {
                    eprintln!("pkaction: unknown option: {arg}");
                    return 1;
                } else {
                    // Treat as action-id filter.
                    action_id_filter = Some(arg.clone());
                }
            }
        }
    }

    let store = PolicyStore::load();
    let mut actions: Vec<&Action> = store.actions.iter().collect();

    // Apply filter.
    if let Some(ref filter) = action_id_filter {
        actions.retain(|a| a.id == *filter || action_pattern_matches(filter, &a.id));
    }

    // Sort by action ID for deterministic output.
    actions.sort_by(|a, b| a.id.cmp(&b.id));

    if actions.is_empty() {
        if let Some(ref filter) = action_id_filter {
            eprintln!("pkaction: no action matching '{filter}'");
        } else {
            eprintln!("pkaction: no actions registered");
        }
        return 1;
    }

    let stdout = io::stdout();
    let mut out = stdout.lock();

    for action in &actions {
        if verbose {
            let _ = writeln!(out, "{}:", action.id);
            if !action.description.is_empty() {
                let _ = writeln!(out, "  description:       {}", action.description);
            }
            if !action.message.is_empty() {
                let _ = writeln!(out, "  message:           {}", action.message);
            }
            if !action.vendor.is_empty() {
                let _ = writeln!(out, "  vendor:            {}", action.vendor);
            }
            if !action.vendor_url.is_empty() {
                let _ = writeln!(out, "  vendor_url:        {}", action.vendor_url);
            }
            if !action.icon_name.is_empty() {
                let _ = writeln!(out, "  icon_name:         {}", action.icon_name);
            }
            let _ = writeln!(out, "  implicit any:      {}", action.defaults_any);
            let _ = writeln!(out, "  implicit inactive: {}", action.defaults_inactive);
            let _ = writeln!(out, "  implicit active:   {}", action.defaults_active);
            if !action.annotations.is_empty() {
                let _ = writeln!(out, "  annotations:");
                let mut keys: Vec<&String> = action.annotations.keys().collect();
                keys.sort();
                for key in keys {
                    if let Some(val) = action.annotations.get(key) {
                        let _ = writeln!(out, "    {key}: {val}");
                    }
                }
            }
            let _ = writeln!(out);
        } else {
            let _ = writeln!(out, "{}", action.id);
        }
    }

    0
}

fn print_pkaction_usage() {
    println!("Usage: pkaction [OPTIONS]");
    println!();
    println!("List registered PolicyKit actions.");
    println!();
    println!("Options:");
    println!("  --action-id ID    Show only the specified action");
    println!("  --verbose, -v     Show detailed information for each action");
    println!("  --version, -V     Print version and exit");
    println!("  --help, -h        Print this help and exit");
}

// ============================================================================
// Personality: pkcheck (check authorization)
// ============================================================================

/// Run the pkcheck personality: check whether a process is authorized.
fn run_pkcheck(args: &[String]) -> i32 {
    let mut action_id: Option<String> = None;
    let mut process_pid: Option<u32> = None;
    let mut allow_user_interaction = false;
    let mut enable_internal_agent = true;

    let mut rest = args;
    while let Some((arg, tail)) = rest.split_first() {
        rest = tail;
        match arg.as_str() {
            "--help" | "-h" => {
                print_pkcheck_usage();
                return 0;
            }
            "--version" | "-V" => {
                println!("pkcheck 0.1.0 (Slate OS)");
                return 0;
            }
            "--action-id" => {
                let Some((value, after_value)) = rest.split_first() else {
                    eprintln!("pkcheck: --action-id requires a value");
                    return 1;
                };
                action_id = Some(value.clone());
                rest = after_value;
            }
            "--process" | "-p" => {
                let Some((value, after_value)) = rest.split_first() else {
                    eprintln!("pkcheck: --process requires a PID");
                    return 1;
                };
                rest = after_value;
                match value.parse::<u32>() {
                    Ok(pid) => process_pid = Some(pid),
                    Err(_) => {
                        eprintln!("pkcheck: invalid PID: {value}");
                        return 1;
                    }
                }
            }
            "--allow-user-interaction" => {
                allow_user_interaction = true;
            }
            "--enable-internal-agent" => {
                enable_internal_agent = true;
            }
            "--disable-internal-agent" => {
                enable_internal_agent = false;
            }
            _ => {
                if let Some(val) = arg.strip_prefix("--action-id=") {
                    action_id = Some(val.to_string());
                } else if let Some(val) = arg.strip_prefix("--process=") {
                    match val.parse::<u32>() {
                        Ok(pid) => process_pid = Some(pid),
                        Err(_) => {
                            eprintln!("pkcheck: invalid PID: {val}");
                            return 1;
                        }
                    }
                } else if arg.starts_with('-') {
                    eprintln!("pkcheck: unknown option: {arg}");
                    return 1;
                } else if action_id.is_none() {
                    // Positional: treat as action-id if not set.
                    action_id = Some(arg.clone());
                }
            }
        }
    }

    let action_id = match action_id {
        Some(id) => id,
        None => {
            eprintln!("pkcheck: --action-id is required");
            return 1;
        }
    };

    // Determine the subject user. If --process is given, look up the UID
    // of that process from /proc/<pid>/status. Otherwise use the caller.
    let users = read_users();
    let subject_uid = if let Some(pid) = process_pid {
        get_process_uid(pid).unwrap_or_else(|| {
            eprintln!("pkcheck: cannot determine UID for PID {pid}");
            u32::MAX
        })
    } else {
        get_caller_uid(&users)
    };

    let subject = match users.find_uid(subject_uid) {
        Some(u) => u.clone(),
        None => {
            eprintln!("pkcheck: unknown subject (uid {subject_uid})");
            return 2;
        }
    };

    let store = PolicyStore::load();
    let result = store.check_authorization(&action_id, &subject, true);

    match result {
        AuthResult::Yes => {
            println!("authorized");
            0
        }
        AuthResult::No => {
            println!("not authorized");
            2
        }
        AuthResult::AuthAdmin => {
            if allow_user_interaction && enable_internal_agent {
                if authenticate_admin(&users) {
                    println!("authorized (after admin auth)");
                    0
                } else {
                    println!("not authorized (admin auth failed)");
                    2
                }
            } else {
                println!("requires admin authentication");
                1
            }
        }
        AuthResult::AuthSelf => {
            if allow_user_interaction && enable_internal_agent {
                if authenticate_self(&subject) {
                    println!("authorized (after self auth)");
                    0
                } else {
                    println!("not authorized (self auth failed)");
                    2
                }
            } else {
                println!("requires self authentication");
                1
            }
        }
    }
}

/// Get the UID of a process by reading `/proc/<pid>/status`.
fn get_process_uid(pid: u32) -> Option<u32> {
    let path = format!("/proc/{pid}/status");
    let content = fs::read_to_string(&path).ok()?;
    for line in content.lines() {
        if let Some(rest) = line.strip_prefix("Uid:")
            && let Some(uid_str) = rest.split_whitespace().next()
        {
            return uid_str.parse().ok();
        }
    }
    None
}

fn print_pkcheck_usage() {
    println!("Usage: pkcheck [OPTIONS]");
    println!();
    println!("Check whether a process is authorized for a PolicyKit action.");
    println!();
    println!("Options:");
    println!("  --action-id ID               Action to check");
    println!("  --process PID, -p PID        Subject process PID");
    println!("  --allow-user-interaction     Allow interactive authentication");
    println!("  --enable-internal-agent      Use built-in auth agent (default)");
    println!("  --disable-internal-agent     Do not use built-in auth agent");
    println!("  --version, -V               Print version and exit");
    println!("  --help, -h                  Print this help and exit");
    println!();
    println!("Exit codes:");
    println!("  0  Authorized");
    println!("  1  Requires authentication (no interaction allowed)");
    println!("  2  Not authorized");
}

// ============================================================================
// Personality detection and dispatch
// ============================================================================

/// Extract the base name from a path (everything after the last `/` or `\`).
fn basename(path: &str) -> &str {
    let after_slash = path.rsplit('/').next().unwrap_or(path);
    after_slash.rsplit('\\').next().unwrap_or(after_slash)
}

/// Detect which personality to run based on argv[0] or a subcommand.
fn detect_personality(args: &[String]) -> &'static str {
    // Check argv[0].
    if let Some(prog) = args.first() {
        let base = basename(prog);
        if base.contains("polkitd") {
            return "polkitd";
        }
        if base.contains("pkexec") {
            return "pkexec";
        }
        if base.contains("pkaction") {
            return "pkaction";
        }
        if base.contains("pkcheck") {
            return "pkcheck";
        }
    }

    // Check for subcommand.
    if let Some(sub) = args.get(1) {
        match sub.as_str() {
            "daemon" | "polkitd" => return "polkitd",
            "exec" | "pkexec" => return "pkexec",
            "action" | "pkaction" => return "pkaction",
            "check" | "pkcheck" => return "pkcheck",
            _ => {}
        }
    }

    // Default to daemon.
    "polkitd"
}

/// Main dispatch function.
fn run_main() -> i32 {
    let args: Vec<String> = env::args().collect();
    let personality = detect_personality(&args);

    // Strip the subcommand if present to get remaining args. `split_first`
    // twice: argv[0] and the subcommand are each present-or-not, and dropping
    // each one where it is proved to exist says so without a length test that
    // could drift from the index it guards.
    let sub_args: Vec<String> = match args.split_first() {
        None => Vec::new(),
        Some((_argv0, after_argv0)) => match after_argv0.split_first() {
            Some((
                sub,
                after_sub,
            )) if matches!(
                sub.as_str(),
                "daemon" | "polkitd" | "exec" | "pkexec" | "action" | "pkaction" | "check"
                    | "pkcheck"
            ) =>
            {
                after_sub.to_vec()
            }
            _ => after_argv0.to_vec(),
        },
    };

    match personality {
        "polkitd" => run_polkitd(&sub_args),
        "pkexec" => run_pkexec(&sub_args),
        "pkaction" => run_pkaction(&sub_args),
        "pkcheck" => run_pkcheck(&sub_args),
        _ => {
            eprintln!("polkit: unknown personality '{personality}'");
            1
        }
    }
}

#[cfg(not(test))]
#[unsafe(no_mangle)]
pub extern "C" fn main(_argc: i32, _argv: *const *const u8) -> i32 {
    run_main()
}

// ============================================================================
// Tests
// ============================================================================

// The workspace's defensive lints are for production code; a test that indexes
// a fixture it just built is asserting, and an assertion that fails by
// panicking is a test doing its job.
#[allow(
    clippy::indexing_slicing,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::arithmetic_side_effects
)]
#[cfg(test)]
mod tests {
    use super::*;

    /// A user record for authorization tests.
    ///
    /// Built through the same setters `useradm` uses rather than from a struct
    /// literal, so a test cannot describe a record the writer could not
    /// produce — which is how this crate came to check `admin:` for years
    /// while every real database spelled it `is_admin:`.
    fn test_user(uid: u32, username: &str, groups: &[&str], admin: bool) -> Record {
        let mut r = Record::new();
        r.set_uid(uid);
        r.set(userdb::field::USERNAME, username);
        let owned: Vec<String> = groups.iter().map(|g| (*g).to_string()).collect();
        r.set_groups(&owned);
        r.set_admin(admin);
        r
    }

    // --- AuthResult ---

    #[test]
    fn test_auth_result_yes_str() {
        assert_eq!(AuthResult::Yes.as_str(), "yes");
    }

    #[test]
    fn test_auth_result_no_str() {
        assert_eq!(AuthResult::No.as_str(), "no");
    }

    #[test]
    fn test_auth_result_admin_str() {
        assert_eq!(AuthResult::AuthAdmin.as_str(), "auth_admin");
    }

    #[test]
    fn test_auth_result_self_str() {
        assert_eq!(AuthResult::AuthSelf.as_str(), "auth_self");
    }

    #[test]
    fn test_auth_result_from_str_yes() {
        assert_eq!(AuthResult::from_str("yes"), Some(AuthResult::Yes));
    }

    #[test]
    fn test_auth_result_from_str_no() {
        assert_eq!(AuthResult::from_str("no"), Some(AuthResult::No));
    }

    #[test]
    fn test_auth_result_from_str_admin() {
        assert_eq!(
            AuthResult::from_str("auth_admin"),
            Some(AuthResult::AuthAdmin)
        );
    }

    #[test]
    fn test_auth_result_from_str_self() {
        assert_eq!(
            AuthResult::from_str("auth_self"),
            Some(AuthResult::AuthSelf)
        );
    }

    #[test]
    fn test_auth_result_from_str_invalid() {
        assert_eq!(AuthResult::from_str("maybe"), None);
    }

    #[test]
    fn test_auth_result_from_str_whitespace() {
        assert_eq!(AuthResult::from_str("  yes  "), Some(AuthResult::Yes));
    }

    #[test]
    fn test_auth_result_display() {
        assert_eq!(format!("{}", AuthResult::AuthAdmin), "auth_admin");
    }

    // --- Action pattern matching ---

    #[test]
    fn test_pattern_exact_match() {
        assert!(action_pattern_matches("org.slateos.foo", "org.slateos.foo"));
    }

    #[test]
    fn test_pattern_exact_no_match() {
        assert!(!action_pattern_matches(
            "org.slateos.foo",
            "org.slateos.bar"
        ));
    }

    #[test]
    fn test_pattern_wildcard_star() {
        assert!(action_pattern_matches("*", "anything.at.all"));
    }

    #[test]
    fn test_pattern_wildcard_dot_star() {
        assert!(action_pattern_matches("org.slateos.*", "org.slateos.foo"));
    }

    #[test]
    fn test_pattern_wildcard_dot_star_nested() {
        assert!(action_pattern_matches(
            "org.slateos.*",
            "org.slateos.foo.bar"
        ));
    }

    #[test]
    fn test_pattern_wildcard_dot_star_exact_prefix() {
        // `org.slateos.*` should match `org.slateos` itself (the prefix without a dot).
        assert!(action_pattern_matches("org.slateos.*", "org.slateos"));
    }

    #[test]
    fn test_pattern_wildcard_dot_star_no_match() {
        assert!(!action_pattern_matches("org.slateos.*", "com.other.foo"));
    }

    // The star here deliberately falls *inside* a component rather than after a
    // dot, which is what separates this pair from the `.*` tests above: it must
    // take the plain `starts_with` branch, not the dot-boundary one. (The
    // literals used to read `org.our*` / `org.ouros.foo`; the OuRoS -> Slate OS
    // rename rewrote the action but not the pattern, since `our` is not `ouros`,
    // and left the positive case asserting a match that cannot happen.)
    #[test]
    fn test_pattern_trailing_star() {
        assert!(action_pattern_matches("org.slat*", "org.slateos.foo"));
    }

    #[test]
    fn test_pattern_trailing_star_no_match() {
        assert!(!action_pattern_matches("org.slat*", "com.other"));
    }

    #[test]
    fn test_pattern_empty_pattern() {
        assert!(!action_pattern_matches("", "org.slateos.foo"));
    }

    #[test]
    fn test_pattern_empty_action() {
        assert!(!action_pattern_matches("org.slateos.foo", ""));
    }

    #[test]
    fn test_pattern_both_empty() {
        assert!(action_pattern_matches("", ""));
    }

    // --- XML parsing ---

    #[test]
    fn test_extract_xml_text_simple() {
        assert_eq!(
            extract_xml_text("<vendor>Slate OS</vendor>", "vendor"),
            Some("Slate OS".to_string())
        );
    }

    #[test]
    fn test_extract_xml_text_no_match() {
        assert_eq!(extract_xml_text("<other>val</other>", "vendor"), None);
    }

    #[test]
    fn test_extract_xml_text_empty_content() {
        assert_eq!(
            extract_xml_text("<description></description>", "description"),
            Some(String::new())
        );
    }

    #[test]
    fn test_extract_action_id() {
        assert_eq!(
            extract_action_id("<action id=\"org.slateos.test\">"),
            Some("org.slateos.test".to_string())
        );
    }

    #[test]
    fn test_extract_action_id_no_match() {
        assert_eq!(extract_action_id("<notaction id=\"x\">"), None);
    }

    #[test]
    fn test_extract_annotate_simple() {
        assert_eq!(
            extract_annotate("<annotate key=\"org.slateos.exec.path\">/usr/bin/foo</annotate>"),
            Some((
                "org.slateos.exec.path".to_string(),
                "/usr/bin/foo".to_string()
            ))
        );
    }

    #[test]
    fn test_extract_annotate_no_match() {
        assert_eq!(extract_annotate("<description>text</description>"), None);
    }

    #[test]
    fn test_parse_policy_xml_full() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<policyconfig>
  <vendor>Slate OS</vendor>
  <vendor_url>https://slateos.example.com</vendor_url>
  <action id="org.slateos.test.action1">
    <description>Test action one</description>
    <message>Auth required for test one</message>
    <icon_name>test-icon</icon_name>
    <defaults>
      <allow_any>no</allow_any>
      <allow_inactive>auth_admin</allow_inactive>
      <allow_active>yes</allow_active>
    </defaults>
    <annotate key="org.slateos.policykit.exec.path">/usr/bin/test1</annotate>
  </action>
  <action id="org.slateos.test.action2">
    <description>Test action two</description>
    <message>Auth required for test two</message>
    <defaults>
      <allow_any>no</allow_any>
      <allow_inactive>no</allow_inactive>
      <allow_active>auth_self</allow_active>
    </defaults>
  </action>
</policyconfig>"#;

        let actions = parse_policy_xml(xml);
        assert_eq!(actions.len(), 2);

        assert_eq!(actions[0].id, "org.slateos.test.action1");
        assert_eq!(actions[0].description, "Test action one");
        assert_eq!(actions[0].message, "Auth required for test one");
        assert_eq!(actions[0].icon_name, "test-icon");
        assert_eq!(actions[0].vendor, "Slate OS");
        assert_eq!(actions[0].vendor_url, "https://slateos.example.com");
        assert_eq!(actions[0].defaults_any, AuthResult::No);
        assert_eq!(actions[0].defaults_inactive, AuthResult::AuthAdmin);
        assert_eq!(actions[0].defaults_active, AuthResult::Yes);
        assert_eq!(
            actions[0]
                .annotations
                .get("org.slateos.policykit.exec.path"),
            Some(&"/usr/bin/test1".to_string())
        );

        assert_eq!(actions[1].id, "org.slateos.test.action2");
        assert_eq!(actions[1].defaults_active, AuthResult::AuthSelf);
    }

    #[test]
    fn test_parse_policy_xml_empty() {
        let xml = "<policyconfig></policyconfig>";
        let actions = parse_policy_xml(xml);
        assert!(actions.is_empty());
    }

    #[test]
    fn test_parse_policy_xml_no_defaults() {
        let xml = r#"<policyconfig>
  <action id="org.slateos.minimal">
    <description>Minimal</description>
  </action>
</policyconfig>"#;

        let actions = parse_policy_xml(xml);
        assert_eq!(actions.len(), 1);
        // Should use default values.
        assert_eq!(actions[0].defaults_active, AuthResult::AuthAdmin);
        assert_eq!(actions[0].defaults_inactive, AuthResult::No);
        assert_eq!(actions[0].defaults_any, AuthResult::No);
    }

    // --- Rules parsing ---

    #[test]
    fn test_parse_rules_basic() {
        let rules_text = r#"
# Allow alice to run anything under org.slateos.pkexec
- action: org.slateos.pkexec.*
  user: alice
  result: yes
  priority: 10
"#;
        let rules = parse_rules_file(rules_text);
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].action_pattern, "org.slateos.pkexec.*");
        assert_eq!(rules[0].user, Some("alice".to_string()));
        assert_eq!(rules[0].group, None);
        assert_eq!(rules[0].result, AuthResult::Yes);
        assert_eq!(rules[0].priority, 10);
    }

    #[test]
    fn test_parse_rules_group() {
        let rules_text = r#"
- action: org.slateos.mount.*
  group: storage
  result: auth_self
  priority: 50
"#;
        let rules = parse_rules_file(rules_text);
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].group, Some("storage".to_string()));
        assert_eq!(rules[0].result, AuthResult::AuthSelf);
    }

    #[test]
    fn test_parse_rules_multiple() {
        let rules_text = r#"
- action: org.slateos.a
  user: bob
  result: yes
  priority: 1

- action: org.slateos.b
  user: charlie
  result: no
  priority: 2
"#;
        let rules = parse_rules_file(rules_text);
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0].action_pattern, "org.slateos.a");
        assert_eq!(rules[1].action_pattern, "org.slateos.b");
    }

    #[test]
    fn test_parse_rules_empty() {
        let rules = parse_rules_file("");
        assert!(rules.is_empty());
    }

    #[test]
    fn test_parse_rules_comments_only() {
        let rules_text = "# Just a comment\n# Another comment\n";
        let rules = parse_rules_file(rules_text);
        assert!(rules.is_empty());
    }

    #[test]
    fn test_parse_rules_default_priority() {
        let rules_text = "- action: org.slateos.x\n  result: yes\n";
        let rules = parse_rules_file(rules_text);
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].priority, 100); // default
    }

    #[test]
    fn test_parse_rules_default_result() {
        let rules_text = "- action: org.slateos.x\n";
        let rules = parse_rules_file(rules_text);
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].result, AuthResult::No); // default
    }

    // --- Rule matching ---

    #[test]
    fn test_rule_matches_exact_action() {
        let rule = Rule {
            action_pattern: "org.slateos.foo".to_string(),
            user: None,
            group: None,
            result: AuthResult::Yes,
            priority: 0,
        };
        assert!(rule.matches_action("org.slateos.foo"));
        assert!(!rule.matches_action("org.slateos.bar"));
    }

    #[test]
    fn test_rule_matches_wildcard_action() {
        let rule = Rule {
            action_pattern: "org.slateos.*".to_string(),
            user: None,
            group: None,
            result: AuthResult::Yes,
            priority: 0,
        };
        assert!(rule.matches_action("org.slateos.foo"));
        assert!(rule.matches_action("org.slateos.bar.baz"));
        assert!(!rule.matches_action("com.other"));
    }

    // --- Authorization checking ---

    #[test]
    fn test_check_auth_explicit_rule_user_match() {
        let store = PolicyStore {
            actions: vec![],
            rules: vec![Rule {
                action_pattern: "org.slateos.test".to_string(),
                user: Some("alice".to_string()),
                group: None,
                result: AuthResult::Yes,
                priority: 0,
            }],
        };

        let user = test_user(1000, "alice", &["users"], false);

        assert_eq!(
            store.check_authorization("org.slateos.test", &user, true),
            AuthResult::Yes
        );
    }

    #[test]
    fn test_check_auth_explicit_rule_user_no_match() {
        let store = PolicyStore {
            actions: vec![],
            rules: vec![Rule {
                action_pattern: "org.slateos.test".to_string(),
                user: Some("alice".to_string()),
                group: None,
                result: AuthResult::Yes,
                priority: 0,
            }],
        };

        let user = test_user(1001, "bob", &["users"], false);

        // No matching rule, no matching action -> No.
        assert_eq!(
            store.check_authorization("org.slateos.test", &user, true),
            AuthResult::No
        );
    }

    #[test]
    fn test_check_auth_explicit_rule_group_match() {
        let store = PolicyStore {
            actions: vec![],
            rules: vec![Rule {
                action_pattern: "org.slateos.mount.*".to_string(),
                user: None,
                group: Some("storage".to_string()),
                result: AuthResult::AuthSelf,
                priority: 0,
            }],
        };

        let user = test_user(1000, "alice", &["users", "storage"], false);

        assert_eq!(
            store.check_authorization("org.slateos.mount.disk", &user, true),
            AuthResult::AuthSelf
        );
    }

    #[test]
    fn test_check_auth_explicit_rule_group_no_match() {
        let store = PolicyStore {
            actions: vec![],
            rules: vec![Rule {
                action_pattern: "org.slateos.mount.*".to_string(),
                user: None,
                group: Some("storage".to_string()),
                result: AuthResult::AuthSelf,
                priority: 0,
            }],
        };

        let user = test_user(1000, "alice", &["users"], false);

        assert_eq!(
            store.check_authorization("org.slateos.mount.disk", &user, true),
            AuthResult::No
        );
    }

    #[test]
    fn test_check_auth_falls_back_to_action_defaults_active() {
        let store = PolicyStore {
            actions: vec![Action {
                id: "org.slateos.test".to_string(),
                description: "Test".to_string(),
                message: String::new(),
                icon_name: String::new(),
                defaults_inactive: AuthResult::No,
                defaults_active: AuthResult::AuthAdmin,
                defaults_any: AuthResult::No,
                vendor: String::new(),
                vendor_url: String::new(),
                annotations: HashMap::new(),
            }],
            rules: vec![],
        };

        let user = test_user(1000, "bob", &[], false);

        assert_eq!(
            store.check_authorization("org.slateos.test", &user, true),
            AuthResult::AuthAdmin
        );
    }

    #[test]
    fn test_check_auth_falls_back_to_action_defaults_inactive() {
        let store = PolicyStore {
            actions: vec![Action {
                id: "org.slateos.test".to_string(),
                description: "Test".to_string(),
                message: String::new(),
                icon_name: String::new(),
                defaults_inactive: AuthResult::AuthSelf,
                defaults_active: AuthResult::Yes,
                defaults_any: AuthResult::No,
                vendor: String::new(),
                vendor_url: String::new(),
                annotations: HashMap::new(),
            }],
            rules: vec![],
        };

        let user = test_user(1000, "bob", &[], false);

        assert_eq!(
            store.check_authorization("org.slateos.test", &user, false),
            AuthResult::AuthSelf
        );
    }

    #[test]
    fn test_check_auth_unknown_action_denies() {
        let store = PolicyStore {
            actions: vec![],
            rules: vec![],
        };

        let user = test_user(1000, "bob", &[], false);

        assert_eq!(
            store.check_authorization("org.slateos.nonexistent", &user, true),
            AuthResult::No
        );
    }

    #[test]
    fn test_check_auth_rule_priority_order() {
        // Lower priority should win (evaluated first).
        let store = PolicyStore {
            actions: vec![],
            rules: vec![
                Rule {
                    action_pattern: "org.slateos.test".to_string(),
                    user: None,
                    group: None,
                    result: AuthResult::No,
                    priority: 50,
                },
                Rule {
                    action_pattern: "org.slateos.test".to_string(),
                    user: None,
                    group: None,
                    result: AuthResult::Yes,
                    priority: 10, // Lower = checked first
                },
            ],
        };

        let mut sorted_rules = store.rules.clone();
        sorted_rules.sort_by_key(|r| r.priority);

        let store = PolicyStore {
            actions: vec![],
            rules: sorted_rules,
        };

        let user = test_user(1000, "alice", &[], false);

        assert_eq!(
            store.check_authorization("org.slateos.test", &user, true),
            AuthResult::Yes
        );
    }

    #[test]
    fn test_check_auth_wildcard_rule() {
        let store = PolicyStore {
            actions: vec![],
            rules: vec![Rule {
                action_pattern: "*".to_string(),
                user: Some("root".to_string()),
                group: None,
                result: AuthResult::Yes,
                priority: 0,
            }],
        };

        let root = test_user(0, "root", &["root"], true);

        assert_eq!(
            store.check_authorization("anything.at.all", &root, true),
            AuthResult::Yes
        );
    }

    // --- Passwords ---
    //
    // The six tests that were here checked SHA-256 against three published
    // vectors and then checked that `hash_password` was the SHA-256 of the
    // salt concatenated with the password. Every one of them passed. What none
    // of them asked was whether the *writer* of `/etc/users.yaml` produced that
    // composition — it never has — so the crate authenticated nobody while its
    // password tests were entirely green. The replacements below go through the
    // file, which is the only place the two sides can disagree.

    /// A database as `useradm` writes one, including the field spellings this
    /// crate used to get wrong.
    fn auth_fixture() -> UserDb {
        let mut db = UserDb::parse(
            "users:\n  \
             - uid: 1000\n    \
             username: \"alice\"\n    \
             is_admin: true\n    \
             home_dir: \"/home/alice\"\n  \
             - uid: 1001\n    \
             username: \"bob\"\n    \
             is_admin: false\n",
        );
        let alice = db.find_mut("alice").expect("fixture has alice");
        alice
            .set_password_with_salt("hunter2", "0123456789abcdef")
            .expect("the salt is one crypt can store");
        db
    }

    #[test]
    fn a_password_written_by_useradm_is_accepted() {
        let db = auth_fixture();
        let alice = db.find("alice").expect("fixture has alice");
        assert_eq!(alice.check_password("hunter2"), Auth::Accepted);
        assert_eq!(alice.check_password("hunter3"), Auth::Rejected);
    }

    #[test]
    fn a_password_survives_the_round_trip_through_the_file() {
        // The defect this crate carried was a disagreement between writer and
        // reader, so the test that would have caught it has to serialise.
        let text = auth_fixture().to_text();
        let db = UserDb::parse(&text);
        let alice = db.find("alice").expect("re-parsed database has alice");
        assert_eq!(alice.check_password("hunter2"), Auth::Accepted);
    }

    #[test]
    fn the_admin_flag_is_read_from_the_spelling_useradm_writes() {
        // `is_admin:`, which the old parser ignored in favour of `admin:` — so
        // it saw a database with no administrators and refused every
        // `auth_admin` action before reaching a password prompt.
        let db = auth_fixture();
        assert!(db.find("alice").expect("has alice").is_admin());
        assert!(!db.find("bob").expect("has bob").is_admin());
    }

    #[test]
    fn an_account_with_no_password_cannot_authenticate() {
        let db = auth_fixture();
        let bob = db.find("bob").expect("fixture has bob");
        assert_eq!(bob.check_password(""), Auth::NoPassword);
        assert_eq!(bob.check_password("anything"), Auth::NoPassword);
    }

    #[test]
    fn a_locked_account_refuses_its_own_password() {
        let mut db = auth_fixture();
        db.find_mut("alice").expect("has alice").set_locked(true);
        let alice = db.find("alice").expect("has alice");
        assert_eq!(alice.check_password("hunter2"), Auth::Locked);
    }

    #[test]
    fn an_admin_matches_a_wheel_group_rule() {
        // The database records administrator-ness as a flag, but every polkit
        // rule in the wild is written against the group.
        let store = PolicyStore {
            actions: vec![],
            rules: vec![Rule {
                action_pattern: "org.slateos.test".to_string(),
                user: None,
                group: Some("wheel".to_string()),
                result: AuthResult::Yes,
                priority: 0,
            }],
        };
        let admin = test_user(1000, "alice", &["users"], true);
        let plain = test_user(1001, "bob", &["users"], false);

        assert_eq!(
            store.check_authorization("org.slateos.test", &admin, true),
            AuthResult::Yes
        );
        assert_eq!(
            store.check_authorization("org.slateos.test", &plain, true),
            AuthResult::No
        );
    }

    // --- Personality detection ---

    #[test]
    fn test_detect_polkitd_argv0() {
        let args = vec!["polkitd".to_string()];
        assert_eq!(detect_personality(&args), "polkitd");
    }

    #[test]
    fn test_detect_pkexec_argv0() {
        let args = vec!["/usr/bin/pkexec".to_string()];
        assert_eq!(detect_personality(&args), "pkexec");
    }

    #[test]
    fn test_detect_pkaction_argv0() {
        let args = vec!["pkaction".to_string()];
        assert_eq!(detect_personality(&args), "pkaction");
    }

    #[test]
    fn test_detect_pkcheck_argv0() {
        let args = vec!["/usr/local/bin/pkcheck".to_string()];
        assert_eq!(detect_personality(&args), "pkcheck");
    }

    #[test]
    fn test_detect_daemon_subcommand() {
        let args = vec!["polkit".to_string(), "daemon".to_string()];
        assert_eq!(detect_personality(&args), "polkitd");
    }

    #[test]
    fn test_detect_exec_subcommand() {
        let args = vec!["polkit".to_string(), "exec".to_string()];
        assert_eq!(detect_personality(&args), "pkexec");
    }

    #[test]
    fn test_detect_action_subcommand() {
        let args = vec!["polkit".to_string(), "action".to_string()];
        assert_eq!(detect_personality(&args), "pkaction");
    }

    #[test]
    fn test_detect_check_subcommand() {
        let args = vec!["polkit".to_string(), "check".to_string()];
        assert_eq!(detect_personality(&args), "pkcheck");
    }

    #[test]
    fn test_detect_default_is_polkitd() {
        let args = vec!["polkit".to_string()];
        assert_eq!(detect_personality(&args), "polkitd");
    }

    #[test]
    fn test_detect_windows_path() {
        let args = vec!["C:\\Program Files\\polkit\\pkexec.exe".to_string()];
        assert_eq!(detect_personality(&args), "pkexec");
    }

    // --- basename ---

    #[test]
    fn test_basename_simple() {
        assert_eq!(basename("pkexec"), "pkexec");
    }

    #[test]
    fn test_basename_unix_path() {
        assert_eq!(basename("/usr/bin/polkitd"), "polkitd");
    }

    #[test]
    fn test_basename_windows_path() {
        assert_eq!(basename("C:\\bin\\pkcheck.exe"), "pkcheck.exe");
    }

    #[test]
    fn test_basename_mixed_separators() {
        assert_eq!(basename("/usr/bin\\pkaction"), "pkaction");
    }

    // --- Action construction ---

    #[test]
    fn test_action_new_defaults() {
        let action = Action::new("org.slateos.test");
        assert_eq!(action.id, "org.slateos.test");
        assert_eq!(action.defaults_active, AuthResult::AuthAdmin);
        assert_eq!(action.defaults_inactive, AuthResult::No);
        assert_eq!(action.defaults_any, AuthResult::No);
        assert!(action.description.is_empty());
        assert!(action.annotations.is_empty());
    }

    // --- Safe env vars ---

    #[test]
    fn test_safe_env_vars_contains_path() {
        assert!(SAFE_ENV_VARS.contains(&"PATH"));
    }

    #[test]
    fn test_safe_env_vars_contains_term() {
        assert!(SAFE_ENV_VARS.contains(&"TERM"));
    }

    #[test]
    fn test_safe_env_vars_no_ld_preload() {
        assert!(!SAFE_ENV_VARS.contains(&"LD_PRELOAD"));
    }

    #[test]
    fn test_safe_env_vars_no_ld_library_path() {
        assert!(!SAFE_ENV_VARS.contains(&"LD_LIBRARY_PATH"));
    }

    // --- Policy XML edge cases ---

    #[test]
    fn test_parse_policy_xml_multiple_annotations() {
        let xml = r#"<policyconfig>
  <action id="org.slateos.multi">
    <description>Multi-annotated</description>
    <defaults>
      <allow_active>yes</allow_active>
    </defaults>
    <annotate key="key1">value1</annotate>
    <annotate key="key2">value2</annotate>
    <annotate key="key3">value3</annotate>
  </action>
</policyconfig>"#;

        let actions = parse_policy_xml(xml);
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].annotations.len(), 3);
        assert_eq!(
            actions[0].annotations.get("key1"),
            Some(&"value1".to_string())
        );
        assert_eq!(
            actions[0].annotations.get("key2"),
            Some(&"value2".to_string())
        );
        assert_eq!(
            actions[0].annotations.get("key3"),
            Some(&"value3".to_string())
        );
    }

    #[test]
    fn test_parse_policy_xml_vendor_inheritance() {
        let xml = r#"<policyconfig>
  <vendor>MyVendor</vendor>
  <vendor_url>https://example.com</vendor_url>
  <action id="org.slateos.a">
    <description>A</description>
  </action>
  <action id="org.slateos.b">
    <description>B</description>
  </action>
</policyconfig>"#;

        let actions = parse_policy_xml(xml);
        assert_eq!(actions.len(), 2);
        // Both actions should inherit the vendor info.
        assert_eq!(actions[0].vendor, "MyVendor");
        assert_eq!(actions[0].vendor_url, "https://example.com");
        assert_eq!(actions[1].vendor, "MyVendor");
        assert_eq!(actions[1].vendor_url, "https://example.com");
    }

    // --- Rule with both user and group ---

    #[test]
    fn test_rule_user_and_group_both_specified() {
        // When both user and group are specified, both must match.
        let store = PolicyStore {
            actions: vec![],
            rules: vec![Rule {
                action_pattern: "org.slateos.test".to_string(),
                user: Some("alice".to_string()),
                group: Some("admin".to_string()),
                result: AuthResult::Yes,
                priority: 0,
            }],
        };

        let alice_admin = test_user(1000, "alice", &["admin"], true);

        let alice_no_admin = test_user(1000, "alice", &["users"], false);

        let bob_admin = test_user(1001, "bob", &["admin"], true);

        assert_eq!(
            store.check_authorization("org.slateos.test", &alice_admin, true),
            AuthResult::Yes
        );
        assert_eq!(
            store.check_authorization("org.slateos.test", &alice_no_admin, true),
            AuthResult::No
        );
        assert_eq!(
            store.check_authorization("org.slateos.test", &bob_admin, true),
            AuthResult::No
        );
    }

    // --- PolicyStore find_action ---

    #[test]
    fn test_find_action_found() {
        let store = PolicyStore {
            actions: vec![Action::new("org.slateos.a"), Action::new("org.slateos.b")],
            rules: vec![],
        };
        assert!(store.find_action("org.slateos.a").is_some());
        assert_eq!(
            store.find_action("org.slateos.a").unwrap().id,
            "org.slateos.a"
        );
    }

    #[test]
    fn test_find_action_not_found() {
        let store = PolicyStore {
            actions: vec![Action::new("org.slateos.a")],
            rules: vec![],
        };
        assert!(store.find_action("org.slateos.missing").is_none());
    }

    // --- Edge case: unclosed action tag in XML ---

    #[test]
    fn test_parse_policy_xml_unclosed_action() {
        let xml = r#"<policyconfig>
  <action id="org.slateos.unclosed">
    <description>Unclosed action</description>
"#;
        let actions = parse_policy_xml(xml);
        // Should still capture the action (lenient parsing).
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].id, "org.slateos.unclosed");
    }
}
