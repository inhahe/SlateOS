//! OurOS PolicyKit Authorization Framework
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

use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::{self, Write as IoWrite};

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
    /// Unique action identifier, e.g. `org.ouros.pkexec.run-program`.
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
    /// Action ID pattern. Supports trailing wildcards: `org.ouros.*` matches
    /// any action starting with `org.ouros.`.
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
/// - Exact match: `org.ouros.foo` matches `org.ouros.foo`
/// - Trailing wildcard: `org.ouros.*` matches `org.ouros.foo` and `org.ouros.bar.baz`
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

/// Minimal user record for authorization decisions.
#[derive(Debug, Clone)]
struct UserInfo {
    uid: u32,
    username: String,
    groups: Vec<String>,
    admin: bool,
    password_hash: String,
    salt: String,
}

const USER_DB_PATH: &str = "/etc/users.yaml";

/// Read all users from `/etc/users.yaml`.
fn read_users() -> Vec<UserInfo> {
    let content = match fs::read_to_string(USER_DB_PATH) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    let mut users = Vec::new();
    let mut current: Option<UserInfo> = None;

    for line in content.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with("- uid:") || trimmed.starts_with("-  uid:") {
            if let Some(user) = current.take() {
                users.push(user);
            }
            let uid: u32 = trimmed
                .split(':')
                .nth(1)
                .and_then(|s| s.trim().parse().ok())
                .unwrap_or(0);
            current = Some(UserInfo {
                uid,
                username: String::new(),
                groups: Vec::new(),
                admin: false,
                password_hash: String::new(),
                salt: String::new(),
            });
        } else if let Some(ref mut user) = current {
            if let Some(val) = trimmed.strip_prefix("username:") {
                user.username = val.trim().trim_matches('"').to_string();
            } else if let Some(val) = trimmed.strip_prefix("password_hash:") {
                user.password_hash = val.trim().trim_matches('"').to_string();
            } else if let Some(val) = trimmed.strip_prefix("salt:") {
                user.salt = val.trim().trim_matches('"').to_string();
            } else if let Some(val) = trimmed.strip_prefix("groups:") {
                let val = val.trim().trim_matches(|c: char| c == '[' || c == ']');
                user.groups = val
                    .split(',')
                    .map(|g| g.trim().trim_matches('"').to_string())
                    .filter(|g| !g.is_empty())
                    .collect();
            } else if let Some(val) = trimmed.strip_prefix("admin:") {
                user.admin = val.trim() == "true";
            }
        }
    }

    if let Some(user) = current {
        users.push(user);
    }

    users
}

/// Look up a user by username.
fn find_user<'a>(users: &'a [UserInfo], name: &str) -> Option<&'a UserInfo> {
    users.iter().find(|u| u.username == name)
}

/// Look up a user by UID.
fn find_user_by_uid<'a>(users: &'a [UserInfo], uid: u32) -> Option<&'a UserInfo> {
    users.iter().find(|u| u.uid == uid)
}

/// Get the current user's UID from `/proc/self/status` or the USER env var.
fn get_caller_uid(users: &[UserInfo]) -> u32 {
    if let Ok(content) = fs::read_to_string("/proc/self/status") {
        for line in content.lines() {
            if let Some(rest) = line.strip_prefix("Uid:") {
                if let Some(uid_str) = rest.trim().split_whitespace().next() {
                    if let Ok(uid) = uid_str.parse::<u32>() {
                        return uid;
                    }
                }
            }
        }
    }

    if let Ok(name) = env::var("USER") {
        if let Some(user) = find_user(users, &name) {
            return user.uid;
        }
    }

    u32::MAX
}

// ============================================================================
// SHA-256 (for admin authentication -- matches su/useradm)
// ============================================================================

/// SHA-256 hash returning a lowercase hex string.
fn sha256_hex(data: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5,
        0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
        0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3,
        0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
        0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc,
        0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
        0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
        0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
        0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
        0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3,
        0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
        0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5,
        0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208,
        0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];

    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a,
        0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
    ];

    let bit_len = (data.len() as u64).wrapping_mul(8);
    let mut padded = data.to_vec();
    padded.push(0x80);
    while (padded.len() % 64) != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in padded.chunks(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                chunk[i * 4],
                chunk[i * 4 + 1],
                chunk[i * 4 + 2],
                chunk[i * 4 + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7)
                ^ w[i - 15].rotate_right(18)
                ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17)
                ^ w[i - 2].rotate_right(19)
                ^ (w[i - 2] >> 10);
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
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0_val = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0_val.wrapping_add(maj);

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

/// Hash a password with the given salt.
fn hash_password(password: &str, salt: &str) -> String {
    let input = format!("{salt}{password}");
    sha256_hex(input.as_bytes())
}

/// Verify a password against a stored hash and salt (constant-time).
fn verify_password(password: &str, stored_hash: &str, salt: &str) -> bool {
    let computed = hash_password(password, salt);
    if computed.len() != stored_hash.len() {
        return false;
    }
    let mut diff: u8 = 0;
    for (a, b) in computed.bytes().zip(stored_hash.bytes()) {
        diff |= a ^ b;
    }
    diff == 0
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
///   <vendor>OurOS</vendor>
///   <vendor_url>https://ouros.example.com</vendor_url>
///   <action id="org.ouros.example">
///     <description>Do something</description>
///     <message>Authentication is required to do something</message>
///     <icon_name>dialog-password</icon_name>
///     <defaults>
///       <allow_any>no</allow_any>
///       <allow_inactive>no</allow_inactive>
///       <allow_active>auth_admin</allow_active>
///     </defaults>
///     <annotate key="org.ouros.policykit.exec.path">/usr/bin/something</annotate>
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
                } else if let Some(val) = extract_xml_text(trimmed, "allow_active") {
                    if let Some(r) = AuthResult::from_str(&val) {
                        action.defaults_active = r;
                    }
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
    if let Some(rest) = line.strip_prefix(&open) {
        if let Some(text) = rest.strip_suffix(&close) {
            return Some(text.to_string());
        }
    }
    None
}

/// Extract action id from `<action id="...">`.
fn extract_action_id(line: &str) -> Option<String> {
    let prefix = "<action id=\"";
    if let Some(rest) = line.strip_prefix(prefix) {
        if let Some(end_quote) = rest.find('"') {
            return Some(rest[..end_quote].to_string());
        }
    }
    None
}

/// Extract an annotation: `<annotate key="key">value</annotate>`.
fn extract_annotate(line: &str) -> Option<(String, String)> {
    let prefix = "<annotate key=\"";
    if let Some(rest) = line.strip_prefix(prefix) {
        if let Some(end_quote) = rest.find('"') {
            let key = rest[..end_quote].to_string();
            let after_key = &rest[end_quote + 1..];
            // Skip the `>`
            if let Some(after_gt) = after_key.strip_prefix('>') {
                if let Some(val_end) = after_gt.find("</annotate>") {
                    let val = after_gt[..val_end].to_string();
                    return Some((key, val));
                }
            }
        }
    }
    None
}

// ============================================================================
// Rules parser (YAML-like declarative format)
// ============================================================================

/// Directories containing authorization rules.
const RULES_DIRS: &[&str] = &[
    "/etc/polkit-1/rules.d",
    "/usr/share/polkit-1/rules.d",
];

/// Parse a rules file.
///
/// Format (one rule per YAML document-like block):
/// ```yaml
/// - action: org.ouros.pkexec.*
///   user: alice
///   result: yes
///   priority: 10
///
/// - action: org.ouros.mount.*
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
            } else if let Some(val) = trimmed.strip_prefix("priority:") {
                if let Ok(p) = val.trim().parse::<i32>() {
                    rule.priority = p;
                }
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
                if path.extension().and_then(|e| e.to_str()) == Some("policy") {
                    if let Ok(content) = fs::read_to_string(&path) {
                        actions.extend(parse_policy_xml(&content));
                    }
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
        user: &UserInfo,
        is_active_session: bool,
    ) -> AuthResult {
        // 1. Explicit rules.
        for rule in &self.rules {
            if !rule.matches_action(action_id) {
                continue;
            }

            // Check user constraint.
            if let Some(ref rule_user) = rule.user {
                if *rule_user != user.username {
                    continue;
                }
            }

            // Check group constraint.
            if let Some(ref rule_group) = rule.group {
                if !user.groups.iter().any(|g| g == rule_group) {
                    continue;
                }
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
// Admin authentication
// ============================================================================

/// Prompt an admin user to authenticate.
///
/// Tries to find any admin user in the database. In a real system this
/// would pop up an authentication agent; here we read from stdin.
fn authenticate_admin(users: &[UserInfo]) -> bool {
    let admins: Vec<&UserInfo> = users.iter().filter(|u| u.admin).collect();
    if admins.is_empty() {
        eprintln!("polkit: no admin users configured");
        return false;
    }

    let _ = write!(
        io::stderr(),
        "Authentication required. Admin users: {}\n",
        admins
            .iter()
            .map(|u| u.username.as_str())
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

    let admin = match admins.iter().find(|u| u.username == username) {
        Some(a) => a,
        None => {
            eprintln!("polkit: user '{username}' is not an admin");
            return false;
        }
    };

    eprint!("Password: ");
    let _ = io::stderr().flush();
    let mut password = String::new();
    if io::stdin().read_line(&mut password).is_err() {
        eprintln!("polkit: failed to read password");
        return false;
    }
    let password = password.trim();

    verify_password(password, &admin.password_hash, &admin.salt)
}

/// Prompt the calling user to authenticate themselves.
fn authenticate_self(user: &UserInfo) -> bool {
    let _ = write!(
        io::stderr(),
        "Authentication required for user '{}'.\n",
        user.username
    );

    eprint!("Password: ");
    let _ = io::stderr().flush();
    let mut password = String::new();
    if io::stdin().read_line(&mut password).is_err() {
        eprintln!("polkit: failed to read password");
        return false;
    }
    let password = password.trim();

    verify_password(password, &user.password_hash, &user.salt)
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

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--no-debug" => no_debug = true,
            "--replace" | "-r" => replace = true,
            "--help" | "-h" => {
                print_polkitd_usage();
                return 0;
            }
            "--version" | "-V" => {
                println!("polkitd 0.1.0 (OurOS)");
                return 0;
            }
            _ => {
                if !args[i].starts_with('-') {
                    foreground = true; // positional: treat as foreground flag
                } else {
                    eprintln!("polkitd: unknown option: {}", args[i]);
                    return 1;
                }
            }
        }
        i += 1;
    }

    // In a daemon, we would fork into the background. On OurOS the service
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
        let parts: Vec<&str> = line.trim().split_whitespace().collect();
        if parts.is_empty() {
            continue;
        }

        match parts[0].to_uppercase().as_str() {
            "CHECK" => {
                if parts.len() < 3 {
                    println!("ERR: usage: CHECK <action_id> <uid>");
                    continue;
                }
                let action_id = parts[1];
                let uid: u32 = match parts[2].parse() {
                    Ok(u) => u,
                    Err(_) => {
                        println!("ERR: invalid uid");
                        continue;
                    }
                };
                let user = match find_user_by_uid(&users, uid) {
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
                println!("ERR: unknown command '{}'", parts[0]);
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
    println!("OurOS PolicyKit authorization daemon.");
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
    "TERM", "COLORTERM", "DISPLAY", "XAUTHORITY", "WAYLAND_DISPLAY",
    "LANG", "LANGUAGE", "LC_ALL", "LC_CTYPE", "LC_MESSAGES",
    "HOME", "USER", "LOGNAME", "SHELL", "PATH",
];

/// Run the pkexec personality: execute a command as another user.
fn run_pkexec(args: &[String]) -> i32 {
    let mut target_user = "root".to_string();
    let mut allow_gui = false;
    let mut disable_internal = false;
    let mut command_args: Vec<String> = Vec::new();
    let mut found_command = false;

    let mut i = 0;
    while i < args.len() {
        if found_command {
            command_args.push(args[i].clone());
            i += 1;
            continue;
        }
        match args[i].as_str() {
            "--help" | "-h" => {
                print_pkexec_usage();
                return 0;
            }
            "--version" | "-V" => {
                println!("pkexec 0.1.0 (OurOS)");
                return 0;
            }
            "--user" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("pkexec: --user requires a username");
                    return 127;
                }
                target_user = args[i].clone();
            }
            "--disable-internal-agent" => {
                disable_internal = true;
            }
            "--keep-cwd" | "--allow-gui" => {
                allow_gui = true;
            }
            _ => {
                if args[i].starts_with('-') {
                    // Check for --user=value form.
                    if let Some(val) = args[i].strip_prefix("--user=") {
                        target_user = val.to_string();
                    } else {
                        eprintln!("pkexec: unknown option: {}", args[i]);
                        return 127;
                    }
                } else {
                    // First non-option argument is the command.
                    command_args.push(args[i].clone());
                    found_command = true;
                }
            }
        }
        i += 1;
    }

    if command_args.is_empty() {
        eprintln!("pkexec: no command specified");
        print_pkexec_usage();
        return 127;
    }

    let _ = allow_gui;
    let _ = disable_internal;

    let users = read_users();
    let caller_uid = get_caller_uid(&users);
    let caller = find_user_by_uid(&users, caller_uid).cloned();

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
    let command_path = &command_args[0];
    let action_id = determine_pkexec_action(command_path);

    let store = PolicyStore::load();
    let result = store.check_authorization(&action_id, &caller, true);

    match result {
        AuthResult::Yes => {
            exec_command(&command_args, &target_user, &users)
        }
        AuthResult::No => {
            eprintln!(
                "pkexec: not authorized to execute '{}' as '{target_user}'",
                command_args[0]
            );
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
/// Looks for a matching annotation `org.ouros.policykit.exec.path` in the
/// loaded actions. Falls back to `org.ouros.policykit.exec`.
fn determine_pkexec_action(command_path: &str) -> String {
    let store = PolicyStore::load();
    for action in &store.actions {
        if let Some(path) = action.annotations.get("org.ouros.policykit.exec.path") {
            if path == command_path {
                return action.id.clone();
            }
        }
    }
    "org.ouros.policykit.exec".to_string()
}

/// Execute a command as the target user with a sanitized environment.
fn exec_command(command_args: &[String], target_user: &str, users: &[UserInfo]) -> i32 {
    let target = find_user(users, target_user);

    // Sanitize environment: only keep safe variables.
    let saved: Vec<(String, String)> = SAFE_ENV_VARS
        .iter()
        .filter_map(|key| env::var(key).ok().map(|val| (key.to_string(), val)))
        .collect();

    // On OurOS we would use exec() to replace the process. Since we cannot
    // exec in this stub environment, we simulate with std::process::Command.
    let program = &command_args[0];
    let program_args = if command_args.len() > 1 {
        &command_args[1..]
    } else {
        &[]
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
        cmd.env("USER", &t.username);
        cmd.env("LOGNAME", &t.username);
        cmd.env("HOME", format!("/home/{}", t.username));
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
            if let Some(rest) = line.strip_prefix("Uid:") {
                if let Some(uid_str) = rest.trim().split_whitespace().next() {
                    return uid_str.to_string();
                }
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

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--verbose" | "-v" => verbose = true,
            "--help" | "-h" => {
                print_pkaction_usage();
                return 0;
            }
            "--version" | "-V" => {
                println!("pkaction 0.1.0 (OurOS)");
                return 0;
            }
            "--action-id" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("pkaction: --action-id requires a value");
                    return 1;
                }
                action_id_filter = Some(args[i].clone());
            }
            _ => {
                if let Some(val) = args[i].strip_prefix("--action-id=") {
                    action_id_filter = Some(val.to_string());
                } else if args[i].starts_with('-') {
                    eprintln!("pkaction: unknown option: {}", args[i]);
                    return 1;
                } else {
                    // Treat as action-id filter.
                    action_id_filter = Some(args[i].clone());
                }
            }
        }
        i += 1;
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

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--help" | "-h" => {
                print_pkcheck_usage();
                return 0;
            }
            "--version" | "-V" => {
                println!("pkcheck 0.1.0 (OurOS)");
                return 0;
            }
            "--action-id" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("pkcheck: --action-id requires a value");
                    return 1;
                }
                action_id = Some(args[i].clone());
            }
            "--process" | "-p" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("pkcheck: --process requires a PID");
                    return 1;
                }
                match args[i].parse::<u32>() {
                    Ok(pid) => process_pid = Some(pid),
                    Err(_) => {
                        eprintln!("pkcheck: invalid PID: {}", args[i]);
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
                if let Some(val) = args[i].strip_prefix("--action-id=") {
                    action_id = Some(val.to_string());
                } else if let Some(val) = args[i].strip_prefix("--process=") {
                    match val.parse::<u32>() {
                        Ok(pid) => process_pid = Some(pid),
                        Err(_) => {
                            eprintln!("pkcheck: invalid PID: {val}");
                            return 1;
                        }
                    }
                } else if args[i].starts_with('-') {
                    eprintln!("pkcheck: unknown option: {}", args[i]);
                    return 1;
                } else {
                    // Positional: treat as action-id if not set.
                    if action_id.is_none() {
                        action_id = Some(args[i].clone());
                    }
                }
            }
        }
        i += 1;
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

    let subject = match find_user_by_uid(&users, subject_uid) {
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
        if let Some(rest) = line.strip_prefix("Uid:") {
            if let Some(uid_str) = rest.trim().split_whitespace().next() {
                return uid_str.parse().ok();
            }
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

    // Strip the subcommand if present to get remaining args.
    let sub_args: Vec<String> = if args.len() > 1
        && matches!(
            args[1].as_str(),
            "daemon" | "polkitd" | "exec" | "pkexec" | "action" | "pkaction" | "check" | "pkcheck"
        )
    {
        args[2..].to_vec()
    } else if args.len() > 1 {
        args[1..].to_vec()
    } else {
        Vec::new()
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

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(AuthResult::from_str("auth_admin"), Some(AuthResult::AuthAdmin));
    }

    #[test]
    fn test_auth_result_from_str_self() {
        assert_eq!(AuthResult::from_str("auth_self"), Some(AuthResult::AuthSelf));
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
        assert!(action_pattern_matches("org.ouros.foo", "org.ouros.foo"));
    }

    #[test]
    fn test_pattern_exact_no_match() {
        assert!(!action_pattern_matches("org.ouros.foo", "org.ouros.bar"));
    }

    #[test]
    fn test_pattern_wildcard_star() {
        assert!(action_pattern_matches("*", "anything.at.all"));
    }

    #[test]
    fn test_pattern_wildcard_dot_star() {
        assert!(action_pattern_matches("org.ouros.*", "org.ouros.foo"));
    }

    #[test]
    fn test_pattern_wildcard_dot_star_nested() {
        assert!(action_pattern_matches("org.ouros.*", "org.ouros.foo.bar"));
    }

    #[test]
    fn test_pattern_wildcard_dot_star_exact_prefix() {
        // `org.ouros.*` should match `org.ouros` itself (the prefix without a dot).
        assert!(action_pattern_matches("org.ouros.*", "org.ouros"));
    }

    #[test]
    fn test_pattern_wildcard_dot_star_no_match() {
        assert!(!action_pattern_matches("org.ouros.*", "com.other.foo"));
    }

    #[test]
    fn test_pattern_trailing_star() {
        assert!(action_pattern_matches("org.our*", "org.ouros.foo"));
    }

    #[test]
    fn test_pattern_trailing_star_no_match() {
        assert!(!action_pattern_matches("org.our*", "com.other"));
    }

    #[test]
    fn test_pattern_empty_pattern() {
        assert!(!action_pattern_matches("", "org.ouros.foo"));
    }

    #[test]
    fn test_pattern_empty_action() {
        assert!(!action_pattern_matches("org.ouros.foo", ""));
    }

    #[test]
    fn test_pattern_both_empty() {
        assert!(action_pattern_matches("", ""));
    }

    // --- XML parsing ---

    #[test]
    fn test_extract_xml_text_simple() {
        assert_eq!(
            extract_xml_text("<vendor>OurOS</vendor>", "vendor"),
            Some("OurOS".to_string())
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
            extract_action_id("<action id=\"org.ouros.test\">"),
            Some("org.ouros.test".to_string())
        );
    }

    #[test]
    fn test_extract_action_id_no_match() {
        assert_eq!(extract_action_id("<notaction id=\"x\">"), None);
    }

    #[test]
    fn test_extract_annotate_simple() {
        assert_eq!(
            extract_annotate("<annotate key=\"org.ouros.exec.path\">/usr/bin/foo</annotate>"),
            Some(("org.ouros.exec.path".to_string(), "/usr/bin/foo".to_string()))
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
  <vendor>OurOS</vendor>
  <vendor_url>https://ouros.example.com</vendor_url>
  <action id="org.ouros.test.action1">
    <description>Test action one</description>
    <message>Auth required for test one</message>
    <icon_name>test-icon</icon_name>
    <defaults>
      <allow_any>no</allow_any>
      <allow_inactive>auth_admin</allow_inactive>
      <allow_active>yes</allow_active>
    </defaults>
    <annotate key="org.ouros.policykit.exec.path">/usr/bin/test1</annotate>
  </action>
  <action id="org.ouros.test.action2">
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

        assert_eq!(actions[0].id, "org.ouros.test.action1");
        assert_eq!(actions[0].description, "Test action one");
        assert_eq!(actions[0].message, "Auth required for test one");
        assert_eq!(actions[0].icon_name, "test-icon");
        assert_eq!(actions[0].vendor, "OurOS");
        assert_eq!(actions[0].vendor_url, "https://ouros.example.com");
        assert_eq!(actions[0].defaults_any, AuthResult::No);
        assert_eq!(actions[0].defaults_inactive, AuthResult::AuthAdmin);
        assert_eq!(actions[0].defaults_active, AuthResult::Yes);
        assert_eq!(
            actions[0].annotations.get("org.ouros.policykit.exec.path"),
            Some(&"/usr/bin/test1".to_string())
        );

        assert_eq!(actions[1].id, "org.ouros.test.action2");
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
  <action id="org.ouros.minimal">
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
# Allow alice to run anything under org.ouros.pkexec
- action: org.ouros.pkexec.*
  user: alice
  result: yes
  priority: 10
"#;
        let rules = parse_rules_file(rules_text);
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].action_pattern, "org.ouros.pkexec.*");
        assert_eq!(rules[0].user, Some("alice".to_string()));
        assert_eq!(rules[0].group, None);
        assert_eq!(rules[0].result, AuthResult::Yes);
        assert_eq!(rules[0].priority, 10);
    }

    #[test]
    fn test_parse_rules_group() {
        let rules_text = r#"
- action: org.ouros.mount.*
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
- action: org.ouros.a
  user: bob
  result: yes
  priority: 1

- action: org.ouros.b
  user: charlie
  result: no
  priority: 2
"#;
        let rules = parse_rules_file(rules_text);
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0].action_pattern, "org.ouros.a");
        assert_eq!(rules[1].action_pattern, "org.ouros.b");
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
        let rules_text = "- action: org.ouros.x\n  result: yes\n";
        let rules = parse_rules_file(rules_text);
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].priority, 100); // default
    }

    #[test]
    fn test_parse_rules_default_result() {
        let rules_text = "- action: org.ouros.x\n";
        let rules = parse_rules_file(rules_text);
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].result, AuthResult::No); // default
    }

    // --- Rule matching ---

    #[test]
    fn test_rule_matches_exact_action() {
        let rule = Rule {
            action_pattern: "org.ouros.foo".to_string(),
            user: None,
            group: None,
            result: AuthResult::Yes,
            priority: 0,
        };
        assert!(rule.matches_action("org.ouros.foo"));
        assert!(!rule.matches_action("org.ouros.bar"));
    }

    #[test]
    fn test_rule_matches_wildcard_action() {
        let rule = Rule {
            action_pattern: "org.ouros.*".to_string(),
            user: None,
            group: None,
            result: AuthResult::Yes,
            priority: 0,
        };
        assert!(rule.matches_action("org.ouros.foo"));
        assert!(rule.matches_action("org.ouros.bar.baz"));
        assert!(!rule.matches_action("com.other"));
    }

    // --- Authorization checking ---

    #[test]
    fn test_check_auth_explicit_rule_user_match() {
        let store = PolicyStore {
            actions: vec![],
            rules: vec![Rule {
                action_pattern: "org.ouros.test".to_string(),
                user: Some("alice".to_string()),
                group: None,
                result: AuthResult::Yes,
                priority: 0,
            }],
        };

        let user = UserInfo {
            uid: 1000,
            username: "alice".to_string(),
            groups: vec!["users".to_string()],
            admin: false,
            password_hash: String::new(),
            salt: String::new(),
        };

        assert_eq!(
            store.check_authorization("org.ouros.test", &user, true),
            AuthResult::Yes
        );
    }

    #[test]
    fn test_check_auth_explicit_rule_user_no_match() {
        let store = PolicyStore {
            actions: vec![],
            rules: vec![Rule {
                action_pattern: "org.ouros.test".to_string(),
                user: Some("alice".to_string()),
                group: None,
                result: AuthResult::Yes,
                priority: 0,
            }],
        };

        let user = UserInfo {
            uid: 1001,
            username: "bob".to_string(),
            groups: vec!["users".to_string()],
            admin: false,
            password_hash: String::new(),
            salt: String::new(),
        };

        // No matching rule, no matching action -> No.
        assert_eq!(
            store.check_authorization("org.ouros.test", &user, true),
            AuthResult::No
        );
    }

    #[test]
    fn test_check_auth_explicit_rule_group_match() {
        let store = PolicyStore {
            actions: vec![],
            rules: vec![Rule {
                action_pattern: "org.ouros.mount.*".to_string(),
                user: None,
                group: Some("storage".to_string()),
                result: AuthResult::AuthSelf,
                priority: 0,
            }],
        };

        let user = UserInfo {
            uid: 1000,
            username: "alice".to_string(),
            groups: vec!["users".to_string(), "storage".to_string()],
            admin: false,
            password_hash: String::new(),
            salt: String::new(),
        };

        assert_eq!(
            store.check_authorization("org.ouros.mount.disk", &user, true),
            AuthResult::AuthSelf
        );
    }

    #[test]
    fn test_check_auth_explicit_rule_group_no_match() {
        let store = PolicyStore {
            actions: vec![],
            rules: vec![Rule {
                action_pattern: "org.ouros.mount.*".to_string(),
                user: None,
                group: Some("storage".to_string()),
                result: AuthResult::AuthSelf,
                priority: 0,
            }],
        };

        let user = UserInfo {
            uid: 1000,
            username: "alice".to_string(),
            groups: vec!["users".to_string()],
            admin: false,
            password_hash: String::new(),
            salt: String::new(),
        };

        assert_eq!(
            store.check_authorization("org.ouros.mount.disk", &user, true),
            AuthResult::No
        );
    }

    #[test]
    fn test_check_auth_falls_back_to_action_defaults_active() {
        let store = PolicyStore {
            actions: vec![Action {
                id: "org.ouros.test".to_string(),
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

        let user = UserInfo {
            uid: 1000,
            username: "bob".to_string(),
            groups: vec![],
            admin: false,
            password_hash: String::new(),
            salt: String::new(),
        };

        assert_eq!(
            store.check_authorization("org.ouros.test", &user, true),
            AuthResult::AuthAdmin
        );
    }

    #[test]
    fn test_check_auth_falls_back_to_action_defaults_inactive() {
        let store = PolicyStore {
            actions: vec![Action {
                id: "org.ouros.test".to_string(),
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

        let user = UserInfo {
            uid: 1000,
            username: "bob".to_string(),
            groups: vec![],
            admin: false,
            password_hash: String::new(),
            salt: String::new(),
        };

        assert_eq!(
            store.check_authorization("org.ouros.test", &user, false),
            AuthResult::AuthSelf
        );
    }

    #[test]
    fn test_check_auth_unknown_action_denies() {
        let store = PolicyStore {
            actions: vec![],
            rules: vec![],
        };

        let user = UserInfo {
            uid: 1000,
            username: "bob".to_string(),
            groups: vec![],
            admin: false,
            password_hash: String::new(),
            salt: String::new(),
        };

        assert_eq!(
            store.check_authorization("org.ouros.nonexistent", &user, true),
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
                    action_pattern: "org.ouros.test".to_string(),
                    user: None,
                    group: None,
                    result: AuthResult::No,
                    priority: 50,
                },
                Rule {
                    action_pattern: "org.ouros.test".to_string(),
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

        let user = UserInfo {
            uid: 1000,
            username: "alice".to_string(),
            groups: vec![],
            admin: false,
            password_hash: String::new(),
            salt: String::new(),
        };

        assert_eq!(
            store.check_authorization("org.ouros.test", &user, true),
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

        let root = UserInfo {
            uid: 0,
            username: "root".to_string(),
            groups: vec!["root".to_string()],
            admin: true,
            password_hash: String::new(),
            salt: String::new(),
        };

        assert_eq!(
            store.check_authorization("anything.at.all", &root, true),
            AuthResult::Yes
        );
    }

    // --- SHA-256 ---

    #[test]
    fn test_sha256_empty() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn test_sha256_hello() {
        assert_eq!(
            sha256_hex(b"hello"),
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    #[test]
    fn test_sha256_abc() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn test_hash_password() {
        let hash = hash_password("secret", "salt123");
        assert_eq!(hash, sha256_hex(b"salt123secret"));
    }

    #[test]
    fn test_verify_password_correct() {
        let hash = hash_password("secret", "salt123");
        assert!(verify_password("secret", &hash, "salt123"));
    }

    #[test]
    fn test_verify_password_wrong() {
        let hash = hash_password("secret", "salt123");
        assert!(!verify_password("wrong", &hash, "salt123"));
    }

    #[test]
    fn test_verify_password_wrong_salt() {
        let hash = hash_password("secret", "salt123");
        assert!(!verify_password("secret", &hash, "other_salt"));
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
        let action = Action::new("org.ouros.test");
        assert_eq!(action.id, "org.ouros.test");
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
  <action id="org.ouros.multi">
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
        assert_eq!(actions[0].annotations.get("key1"), Some(&"value1".to_string()));
        assert_eq!(actions[0].annotations.get("key2"), Some(&"value2".to_string()));
        assert_eq!(actions[0].annotations.get("key3"), Some(&"value3".to_string()));
    }

    #[test]
    fn test_parse_policy_xml_vendor_inheritance() {
        let xml = r#"<policyconfig>
  <vendor>MyVendor</vendor>
  <vendor_url>https://example.com</vendor_url>
  <action id="org.ouros.a">
    <description>A</description>
  </action>
  <action id="org.ouros.b">
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
                action_pattern: "org.ouros.test".to_string(),
                user: Some("alice".to_string()),
                group: Some("admin".to_string()),
                result: AuthResult::Yes,
                priority: 0,
            }],
        };

        let alice_admin = UserInfo {
            uid: 1000,
            username: "alice".to_string(),
            groups: vec!["admin".to_string()],
            admin: true,
            password_hash: String::new(),
            salt: String::new(),
        };

        let alice_no_admin = UserInfo {
            uid: 1000,
            username: "alice".to_string(),
            groups: vec!["users".to_string()],
            admin: false,
            password_hash: String::new(),
            salt: String::new(),
        };

        let bob_admin = UserInfo {
            uid: 1001,
            username: "bob".to_string(),
            groups: vec!["admin".to_string()],
            admin: true,
            password_hash: String::new(),
            salt: String::new(),
        };

        assert_eq!(
            store.check_authorization("org.ouros.test", &alice_admin, true),
            AuthResult::Yes
        );
        assert_eq!(
            store.check_authorization("org.ouros.test", &alice_no_admin, true),
            AuthResult::No
        );
        assert_eq!(
            store.check_authorization("org.ouros.test", &bob_admin, true),
            AuthResult::No
        );
    }

    // --- PolicyStore find_action ---

    #[test]
    fn test_find_action_found() {
        let store = PolicyStore {
            actions: vec![
                Action::new("org.ouros.a"),
                Action::new("org.ouros.b"),
            ],
            rules: vec![],
        };
        assert!(store.find_action("org.ouros.a").is_some());
        assert_eq!(store.find_action("org.ouros.a").unwrap().id, "org.ouros.a");
    }

    #[test]
    fn test_find_action_not_found() {
        let store = PolicyStore {
            actions: vec![Action::new("org.ouros.a")],
            rules: vec![],
        };
        assert!(store.find_action("org.ouros.missing").is_none());
    }

    // --- Edge case: unclosed action tag in XML ---

    #[test]
    fn test_parse_policy_xml_unclosed_action() {
        let xml = r#"<policyconfig>
  <action id="org.ouros.unclosed">
    <description>Unclosed action</description>
"#;
        let actions = parse_policy_xml(xml);
        // Should still capture the action (lenient parsing).
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].id, "org.ouros.unclosed");
    }
}
