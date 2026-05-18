//! OurOS Password Management Utility
//!
//! Manages user passwords and password aging policies via the `/etc/shadow`
//! file. Validates users against `/etc/passwd`.
//!
//! # Usage
//!
//! ```text
//! passwd                       Change own password
//! passwd <username>            Change another user's password (root only)
//! passwd -l <username>         Lock account
//! passwd -u <username>         Unlock account
//! passwd -d <username>         Delete password (passwordless)
//! passwd -S <username>         Show password status
//! passwd -e <username>         Expire password (force change at next login)
//! passwd -n <days> <username>  Minimum password age
//! passwd -x <days> <username>  Maximum password age
//! passwd -w <days> <username>  Warning days before expiry
//! passwd -i <days> <username>  Inactive days after expiry before lock
//! ```
//!
//! # File Formats
//!
//! `/etc/passwd` — colon-separated:
//! `username:x:uid:gid:gecos:home:shell`
//!
//! `/etc/shadow` — colon-separated:
//! `username:hash:lastchanged:min:max:warn:inactive:expire:`

use std::env;
use std::fs;
use std::io::{self, BufRead, Write};
use std::process;

// ============================================================================
// Constants
// ============================================================================

const PASSWD_PATH: &str = "/etc/passwd";
const SHADOW_PATH: &str = "/etc/shadow";
const MIN_PASSWORD_LEN: usize = 8;

/// Number of seconds in a day.
const SECONDS_PER_DAY: u64 = 86400;

// ============================================================================
// Shadow entry model
// ============================================================================

/// Represents one line from `/etc/shadow`.
#[derive(Clone, Debug, PartialEq)]
struct ShadowEntry {
    username: String,
    /// Hashed password. `!` prefix means locked, empty means passwordless.
    hash: String,
    /// Days since epoch when password was last changed.
    last_changed: i64,
    /// Minimum days between password changes (0 = no restriction).
    min_days: i64,
    /// Maximum days a password is valid (-1 = no expiry).
    max_days: i64,
    /// Days before expiry to warn the user.
    warn_days: i64,
    /// Days after expiry before the account is disabled (-1 = never).
    inactive_days: i64,
    /// Days since epoch when account expires (-1 = never).
    expire_date: i64,
}

impl ShadowEntry {
    fn new(username: &str) -> Self {
        ShadowEntry {
            username: username.to_string(),
            hash: String::from("!"),
            last_changed: current_day(),
            min_days: 0,
            max_days: 99999,
            warn_days: 7,
            inactive_days: -1,
            expire_date: -1,
        }
    }

    /// Parse a single shadow line.
    fn parse(line: &str) -> Option<Self> {
        let fields: Vec<&str> = line.split(':').collect();
        if fields.len() < 8 {
            return None;
        }

        Some(ShadowEntry {
            username: fields[0].to_string(),
            hash: fields[1].to_string(),
            last_changed: fields[2].parse().unwrap_or(0),
            min_days: fields[3].parse().unwrap_or(0),
            max_days: fields[4].parse().unwrap_or(99999),
            warn_days: fields[5].parse().unwrap_or(7),
            inactive_days: fields[6].parse().unwrap_or(-1),
            expire_date: if fields.len() > 7 {
                fields[7].parse().unwrap_or(-1)
            } else {
                -1
            },
        })
    }

    /// Serialize back to shadow file format.
    fn to_line(&self) -> String {
        let inactive = if self.inactive_days < 0 {
            String::new()
        } else {
            self.inactive_days.to_string()
        };
        let expire = if self.expire_date < 0 {
            String::new()
        } else {
            self.expire_date.to_string()
        };

        format!(
            "{}:{}:{}:{}:{}:{}:{}:{}:",
            self.username,
            self.hash,
            self.last_changed,
            self.min_days,
            self.max_days,
            self.warn_days,
            inactive,
            expire,
        )
    }

    /// Whether the account is locked (hash starts with `!`).
    fn is_locked(&self) -> bool {
        self.hash.starts_with('!')
    }

    /// Whether the password is empty (passwordless login).
    fn is_passwordless(&self) -> bool {
        self.hash.is_empty()
    }

    /// Status character for `-S` display.
    fn status_char(&self) -> &'static str {
        if self.is_locked() {
            "L"
        } else if self.is_passwordless() {
            "NP"
        } else {
            "P"
        }
    }
}

// ============================================================================
// Passwd entry (read-only, for user validation)
// ============================================================================

/// Minimal `/etc/passwd` entry — we only need the username for validation,
/// but uid is retained for privilege checks.
#[derive(Clone, Debug)]
struct PasswdEntry {
    username: String,
    #[allow(dead_code)]
    uid: u32,
}

impl PasswdEntry {
    fn parse(line: &str) -> Option<Self> {
        let fields: Vec<&str> = line.split(':').collect();
        if fields.len() < 3 {
            return None;
        }
        let uid = fields[2].parse().ok()?;
        Some(PasswdEntry {
            username: fields[0].to_string(),
            uid,
        })
    }
}

// ============================================================================
// File I/O helpers
// ============================================================================

/// Read and parse all shadow entries.
fn read_shadow() -> Vec<ShadowEntry> {
    let content = match fs::read_to_string(SHADOW_PATH) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    content
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(ShadowEntry::parse)
        .collect()
}

/// Write shadow entries back to the file.
fn write_shadow(entries: &[ShadowEntry]) -> Result<(), String> {
    let mut content = String::new();
    for entry in entries {
        content.push_str(&entry.to_line());
        content.push('\n');
    }
    fs::write(SHADOW_PATH, content).map_err(|e| format!("cannot write {SHADOW_PATH}: {e}"))
}

/// Read and parse all passwd entries.
fn read_passwd() -> Vec<PasswdEntry> {
    let content = match fs::read_to_string(PASSWD_PATH) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    content
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(PasswdEntry::parse)
        .collect()
}

/// Find a user in `/etc/passwd` by name.
fn find_user(username: &str) -> Option<PasswdEntry> {
    read_passwd().into_iter().find(|u| u.username == username)
}

/// Find or create a shadow entry for the given username.
fn find_or_create_shadow(entries: &mut Vec<ShadowEntry>, username: &str) -> usize {
    if let Some(idx) = entries.iter().position(|e| e.username == username) {
        idx
    } else {
        entries.push(ShadowEntry::new(username));
        entries.len() - 1
    }
}

// ============================================================================
// SHA-256 implementation
// ============================================================================

/// SHA-256 round constants.
const SHA256_K: [u32; 64] = [
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

/// Compute SHA-256 and return the hex digest string.
fn sha256_hex(data: &[u8]) -> String {
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a,
        0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
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
        for i in 0..16 {
            let base = i * 4;
            w[i] = u32::from_be_bytes([
                chunk[base],
                chunk[base + 1],
                chunk[base + 2],
                chunk[base + 3],
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
// Password hashing and salt generation
// ============================================================================

/// Hash a password with the given salt using SHA-256.
/// Format: `$sha256$<salt>$<hash>`
fn hash_password(password: &str, salt: &str) -> String {
    let input = format!("{salt}${password}");
    let digest = sha256_hex(input.as_bytes());
    format!("$sha256${salt}${digest}")
}

/// Generate a random salt by reading `/dev/urandom`.
fn generate_salt() -> String {
    let mut bytes = [0u8; 16];

    if let Ok(data) = fs::read("/dev/urandom") {
        for (i, b) in data.iter().take(16).enumerate() {
            bytes[i] = *b;
        }
    } else {
        // Fallback: use a time-based seed (not cryptographically ideal).
        let seed = current_day() as u64;
        for (i, b) in bytes.iter_mut().enumerate() {
            let mixed = seed.wrapping_mul(6364136223846793005)
                .wrapping_add(i as u64);
            *b = (mixed >> 32) as u8;
        }
    }

    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Verify a password against a stored hash string.
/// Supports the `$sha256$<salt>$<hash>` format and also bare hex hashes
/// with a separate salt (legacy format from useradm).
fn verify_password(password: &str, stored_hash: &str) -> bool {
    if let Some(rest) = stored_hash.strip_prefix("$sha256$") {
        // Modern format: $sha256$<salt>$<hash>
        if let Some(dollar_pos) = rest.find('$') {
            let salt = &rest[..dollar_pos];
            let expected = hash_password(password, salt);
            return constant_time_eq(stored_hash.as_bytes(), expected.as_bytes());
        }
    }

    // Legacy format: bare hex hash (salt stored separately in users.yaml).
    // Not directly verifiable here without the salt, so return false.
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
// Password strength checking
// ============================================================================

/// Strength check result.
struct StrengthResult {
    ok: bool,
    reasons: Vec<&'static str>,
}

/// Check password strength requirements.
fn check_password_strength(password: &str) -> StrengthResult {
    let mut reasons = Vec::new();

    if password.len() < MIN_PASSWORD_LEN {
        reasons.push("password is too short (minimum 8 characters)");
    }

    let has_upper = password.chars().any(|c| c.is_ascii_uppercase());
    let has_lower = password.chars().any(|c| c.is_ascii_lowercase());
    let has_digit = password.chars().any(|c| c.is_ascii_digit());
    let has_special = password.chars().any(|c| !c.is_ascii_alphanumeric());

    if !has_upper {
        reasons.push("missing uppercase letter");
    }
    if !has_lower {
        reasons.push("missing lowercase letter");
    }
    if !has_digit {
        reasons.push("missing digit");
    }
    if !has_special {
        reasons.push("missing special character");
    }

    // Check for common patterns.
    let lower = password.to_ascii_lowercase();
    if lower.contains("password") || lower.contains("123456") || lower == "qwerty" {
        reasons.push("password contains a common pattern");
    }

    // Check for repeated characters.
    let bytes = password.as_bytes();
    let mut all_same = bytes.len() > 1;
    for window in bytes.windows(2) {
        if window[0] != window[1] {
            all_same = false;
            break;
        }
    }
    if all_same && !bytes.is_empty() {
        reasons.push("password is all the same character");
    }

    StrengthResult {
        ok: reasons.is_empty(),
        reasons,
    }
}

// ============================================================================
// Terminal helpers
// ============================================================================

/// Read a password from stdin without echoing.
/// On OurOS, we disable echo via ioctl on /dev/tty.
/// Falls back to normal line read if terminal control is unavailable.
fn read_password_no_echo(prompt: &str) -> Result<String, String> {
    eprint!("{prompt}");
    let _ = io::stderr().flush();

    // Attempt to disable echo. On OurOS this would use termios ioctls.
    // For now, just read a line — the real echo-disable will be done
    // via the POSIX termios layer when the kernel supports it.
    let mut line = String::new();
    io::stdin()
        .lock()
        .read_line(&mut line)
        .map_err(|e| format!("read error: {e}"))?;
    eprintln!(); // newline after hidden input

    // Trim trailing newline.
    if line.ends_with('\n') {
        line.pop();
    }
    if line.ends_with('\r') {
        line.pop();
    }

    Ok(line)
}

// ============================================================================
// System helpers
// ============================================================================

/// Get the current day number since Unix epoch.
fn current_day() -> i64 {
    match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(dur) => (dur.as_secs() / SECONDS_PER_DAY) as i64,
        Err(_) => 0,
    }
}

/// Determine the current user's UID. Reads the `UID` environment variable
/// (set by the login/init process) or defaults to 0 (root) if unset.
fn current_uid() -> u32 {
    env::var("UID")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

/// Determine the current user's username from the `USER` environment variable.
fn current_username() -> Option<String> {
    env::var("USER").ok()
}

/// Check whether the current user is root.
fn is_root() -> bool {
    current_uid() == 0
}

// ============================================================================
// Argument parsing
// ============================================================================

#[derive(Debug)]
enum Action {
    /// Change password (default).
    ChangePassword,
    /// Lock account (`-l`).
    Lock,
    /// Unlock account (`-u`).
    Unlock,
    /// Delete password (`-d`).
    DeletePassword,
    /// Show status (`-S`).
    ShowStatus,
    /// Expire password (`-e`).
    Expire,
    /// Set minimum days (`-n`).
    SetMinDays(i64),
    /// Set maximum days (`-x`).
    SetMaxDays(i64),
    /// Set warning days (`-w`).
    SetWarnDays(i64),
    /// Set inactive days (`-i`).
    SetInactiveDays(i64),
}

struct Args {
    action: Action,
    target_user: Option<String>,
}

fn parse_args(raw: &[String]) -> Result<Args, String> {
    let mut action = Action::ChangePassword;
    let mut target_user: Option<String> = None;
    let mut idx = 1; // skip argv[0]

    while idx < raw.len() {
        let arg = &raw[idx];
        match arg.as_str() {
            "-l" | "--lock" => {
                action = Action::Lock;
                idx += 1;
            }
            "-u" | "--unlock" => {
                action = Action::Unlock;
                idx += 1;
            }
            "-d" | "--delete" => {
                action = Action::DeletePassword;
                idx += 1;
            }
            "-S" | "--status" => {
                action = Action::ShowStatus;
                idx += 1;
            }
            "-e" | "--expire" => {
                action = Action::Expire;
                idx += 1;
            }
            "-n" | "--mindays" => {
                idx += 1;
                if idx >= raw.len() {
                    return Err("option -n requires a numeric argument".to_string());
                }
                let days: i64 = raw[idx]
                    .parse()
                    .map_err(|_| format!("invalid number for -n: {}", raw[idx]))?;
                action = Action::SetMinDays(days);
                idx += 1;
            }
            "-x" | "--maxdays" => {
                idx += 1;
                if idx >= raw.len() {
                    return Err("option -x requires a numeric argument".to_string());
                }
                let days: i64 = raw[idx]
                    .parse()
                    .map_err(|_| format!("invalid number for -x: {}", raw[idx]))?;
                action = Action::SetMaxDays(days);
                idx += 1;
            }
            "-w" | "--warndays" => {
                idx += 1;
                if idx >= raw.len() {
                    return Err("option -w requires a numeric argument".to_string());
                }
                let days: i64 = raw[idx]
                    .parse()
                    .map_err(|_| format!("invalid number for -w: {}", raw[idx]))?;
                action = Action::SetWarnDays(days);
                idx += 1;
            }
            "-i" | "--inactive" => {
                idx += 1;
                if idx >= raw.len() {
                    return Err("option -i requires a numeric argument".to_string());
                }
                let days: i64 = raw[idx]
                    .parse()
                    .map_err(|_| format!("invalid number for -i: {}", raw[idx]))?;
                action = Action::SetInactiveDays(days);
                idx += 1;
            }
            "-h" | "--help" => {
                print_usage();
                process::exit(0);
            }
            other => {
                if other.starts_with('-') {
                    return Err(format!("unknown option: {other}"));
                }
                if target_user.is_some() {
                    return Err(format!("unexpected argument: {other}"));
                }
                target_user = Some(other.to_string());
                idx += 1;
            }
        }
    }

    Ok(Args {
        action,
        target_user,
    })
}

fn print_usage() {
    eprintln!("Usage: passwd [options] [username]");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  -l, --lock       Lock the account");
    eprintln!("  -u, --unlock     Unlock the account");
    eprintln!("  -d, --delete     Delete the password (passwordless)");
    eprintln!("  -S, --status     Show password status");
    eprintln!("  -e, --expire     Expire password (force change at next login)");
    eprintln!("  -n, --mindays N  Minimum days between changes");
    eprintln!("  -x, --maxdays N  Maximum days before change required");
    eprintln!("  -w, --warndays N Warning days before expiry");
    eprintln!("  -i, --inactive N Inactive days after expiry before lock");
    eprintln!("  -h, --help       Show this help");
}

// ============================================================================
// Command implementations
// ============================================================================

/// Change password for the target user.
fn cmd_change_password(target: &str, caller_uid: u32) -> i32 {
    // Non-root users must verify their current password.
    if caller_uid != 0 {
        let entries = read_shadow();
        if let Some(entry) = entries.iter().find(|e| e.username == target) {
            if entry.is_locked() {
                eprintln!("passwd: account is locked");
                return 1;
            }
            // Check minimum password age.
            if entry.min_days > 0 {
                let days_since = current_day() - entry.last_changed;
                if days_since < entry.min_days {
                    eprintln!(
                        "passwd: password may not be changed yet ({} day(s) remaining)",
                        entry.min_days - days_since
                    );
                    return 1;
                }
            }
            if !entry.hash.is_empty() && !entry.is_locked() {
                let old_pw = match read_password_no_echo("Current password: ") {
                    Ok(p) => p,
                    Err(e) => {
                        eprintln!("passwd: {e}");
                        return 1;
                    }
                };
                if !verify_password(&old_pw, &entry.hash) {
                    eprintln!("passwd: authentication failure");
                    return 1;
                }
            }
        }
    }

    // Read new password.
    let new_pw = match read_password_no_echo("New password: ") {
        Ok(p) => p,
        Err(e) => {
            eprintln!("passwd: {e}");
            return 1;
        }
    };

    // Check strength (only for non-root; root can set weak passwords).
    if caller_uid != 0 {
        let strength = check_password_strength(&new_pw);
        if !strength.ok {
            eprintln!("passwd: password does not meet requirements:");
            for reason in &strength.reasons {
                eprintln!("  - {reason}");
            }
            return 1;
        }
    }

    // Confirm.
    let confirm = match read_password_no_echo("Retype new password: ") {
        Ok(p) => p,
        Err(e) => {
            eprintln!("passwd: {e}");
            return 1;
        }
    };

    if new_pw != confirm {
        eprintln!("passwd: passwords do not match");
        return 1;
    }

    // Hash and store.
    let salt = generate_salt();
    let hashed = hash_password(&new_pw, &salt);

    let mut entries = read_shadow();
    let idx = find_or_create_shadow(&mut entries, target);
    entries[idx].hash = hashed;
    entries[idx].last_changed = current_day();

    if let Err(e) = write_shadow(&entries) {
        eprintln!("passwd: {e}");
        return 1;
    }

    eprintln!("passwd: password updated successfully");
    0
}

/// Lock an account by prepending `!` to the hash.
fn cmd_lock(target: &str) -> i32 {
    let mut entries = read_shadow();
    let idx = find_or_create_shadow(&mut entries, target);

    if entries[idx].is_locked() {
        eprintln!("passwd: account already locked");
        return 0;
    }

    entries[idx].hash = format!("!{}", entries[idx].hash);

    if let Err(e) = write_shadow(&entries) {
        eprintln!("passwd: {e}");
        return 1;
    }

    eprintln!("passwd: account '{}' locked", target);
    0
}

/// Unlock an account by removing the leading `!` from the hash.
fn cmd_unlock(target: &str) -> i32 {
    let mut entries = read_shadow();
    let idx = find_or_create_shadow(&mut entries, target);

    if !entries[idx].is_locked() {
        eprintln!("passwd: account is not locked");
        return 1;
    }

    let hash = &entries[idx].hash;
    if hash == "!" || hash == "!!" {
        eprintln!("passwd: cannot unlock — account has no password set");
        eprintln!("passwd: use passwd -d to remove password or set a new password");
        return 1;
    }

    entries[idx].hash = entries[idx].hash.trim_start_matches('!').to_string();

    if let Err(e) = write_shadow(&entries) {
        eprintln!("passwd: {e}");
        return 1;
    }

    eprintln!("passwd: account '{}' unlocked", target);
    0
}

/// Delete the password (allow passwordless login).
fn cmd_delete_password(target: &str) -> i32 {
    let mut entries = read_shadow();
    let idx = find_or_create_shadow(&mut entries, target);

    entries[idx].hash = String::new();
    entries[idx].last_changed = current_day();

    if let Err(e) = write_shadow(&entries) {
        eprintln!("passwd: {e}");
        return 1;
    }

    eprintln!("passwd: password deleted for '{}'", target);
    0
}

/// Display password status information.
fn cmd_show_status(target: &str) -> i32 {
    let entries = read_shadow();
    let entry = match entries.iter().find(|e| e.username == target) {
        Some(e) => e,
        None => {
            // No shadow entry means no password info.
            println!("{target} NP 1970-01-01 0 99999 7 -1");
            return 0;
        }
    };

    // Compute the date of last change as YYYY-MM-DD.
    let date_str = days_to_date_string(entry.last_changed);

    let inactive_str = if entry.inactive_days < 0 {
        "-1".to_string()
    } else {
        entry.inactive_days.to_string()
    };

    println!(
        "{} {} {} {} {} {} {}",
        target,
        entry.status_char(),
        date_str,
        entry.min_days,
        entry.max_days,
        entry.warn_days,
        inactive_str,
    );

    0
}

/// Expire password — force a change at next login by setting last_changed to 0.
fn cmd_expire(target: &str) -> i32 {
    let mut entries = read_shadow();
    let idx = find_or_create_shadow(&mut entries, target);

    entries[idx].last_changed = 0;

    if let Err(e) = write_shadow(&entries) {
        eprintln!("passwd: {e}");
        return 1;
    }

    eprintln!("passwd: password for '{}' expired", target);
    0
}

/// Set the minimum days between password changes.
fn cmd_set_min_days(target: &str, days: i64) -> i32 {
    let mut entries = read_shadow();
    let idx = find_or_create_shadow(&mut entries, target);

    entries[idx].min_days = days;

    if let Err(e) = write_shadow(&entries) {
        eprintln!("passwd: {e}");
        return 1;
    }

    eprintln!("passwd: minimum password age for '{}' set to {} day(s)", target, days);
    0
}

/// Set the maximum days a password is valid.
fn cmd_set_max_days(target: &str, days: i64) -> i32 {
    let mut entries = read_shadow();
    let idx = find_or_create_shadow(&mut entries, target);

    entries[idx].max_days = days;

    if let Err(e) = write_shadow(&entries) {
        eprintln!("passwd: {e}");
        return 1;
    }

    eprintln!("passwd: maximum password age for '{}' set to {} day(s)", target, days);
    0
}

/// Set the warning days before expiry.
fn cmd_set_warn_days(target: &str, days: i64) -> i32 {
    let mut entries = read_shadow();
    let idx = find_or_create_shadow(&mut entries, target);

    entries[idx].warn_days = days;

    if let Err(e) = write_shadow(&entries) {
        eprintln!("passwd: {e}");
        return 1;
    }

    eprintln!("passwd: warning days for '{}' set to {}", target, days);
    0
}

/// Set the inactive days after expiry before account lock.
fn cmd_set_inactive_days(target: &str, days: i64) -> i32 {
    let mut entries = read_shadow();
    let idx = find_or_create_shadow(&mut entries, target);

    entries[idx].inactive_days = days;

    if let Err(e) = write_shadow(&entries) {
        eprintln!("passwd: {e}");
        return 1;
    }

    eprintln!("passwd: inactive days for '{}' set to {}", target, days);
    0
}

// ============================================================================
// Date helper
// ============================================================================

/// Convert days since epoch to a YYYY-MM-DD string.
fn days_to_date_string(days: i64) -> String {
    if days <= 0 {
        return "1970-01-01".to_string();
    }

    // Simple Gregorian calendar conversion.
    let mut remaining = days as u64;
    let mut year: u64 = 1970;

    loop {
        let days_in_year = if is_leap_year(year) { 366 } else { 365 };
        if remaining < days_in_year {
            break;
        }
        remaining -= days_in_year;
        year += 1;
    }

    let days_in_months: [u64; 12] = if is_leap_year(year) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };

    let mut month: u64 = 1;
    for &dm in &days_in_months {
        if remaining < dm {
            break;
        }
        remaining -= dm;
        month += 1;
    }

    let day = remaining + 1;
    format!("{year:04}-{month:02}-{day:02}")
}

/// Check if a year is a leap year.
fn is_leap_year(year: u64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

// ============================================================================
// Main entry point
// ============================================================================

fn main() {
    let args: Vec<String> = env::args().collect();

    let parsed = match parse_args(&args) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("passwd: {e}");
            print_usage();
            process::exit(1);
        }
    };

    let caller_uid = current_uid();

    // Resolve target user.
    let target = match &parsed.target_user {
        Some(name) => name.clone(),
        None => match current_username() {
            Some(name) => name,
            None => {
                eprintln!("passwd: cannot determine current user");
                process::exit(1);
            }
        },
    };

    // Validate the target user exists in /etc/passwd.
    if find_user(&target).is_none() {
        eprintln!("passwd: user '{}' does not exist", target);
        process::exit(1);
    }

    // Permission check: non-root users can only change their own password
    // (the default ChangePassword action, no flags).
    let changing_own = parsed.target_user.is_none()
        || current_username().as_deref() == Some(target.as_str());

    if !is_root() && !changing_own {
        eprintln!("passwd: only root may change another user's password");
        process::exit(1);
    }

    // Non-ChangePassword actions require root.
    if !is_root() && !matches!(parsed.action, Action::ChangePassword) {
        eprintln!("passwd: only root may use this option");
        process::exit(1);
    }

    let exit_code = match parsed.action {
        Action::ChangePassword => cmd_change_password(&target, caller_uid),
        Action::Lock => cmd_lock(&target),
        Action::Unlock => cmd_unlock(&target),
        Action::DeletePassword => cmd_delete_password(&target),
        Action::ShowStatus => cmd_show_status(&target),
        Action::Expire => cmd_expire(&target),
        Action::SetMinDays(d) => cmd_set_min_days(&target, d),
        Action::SetMaxDays(d) => cmd_set_max_days(&target, d),
        Action::SetWarnDays(d) => cmd_set_warn_days(&target, d),
        Action::SetInactiveDays(d) => cmd_set_inactive_days(&target, d),
    };

    process::exit(exit_code);
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ---- SHA-256 tests ----

    #[test]
    fn sha256_empty_string() {
        let result = sha256_hex(b"");
        assert_eq!(
            result,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn sha256_abc() {
        let result = sha256_hex(b"abc");
        assert_eq!(
            result,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn sha256_longer_input() {
        let result = sha256_hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq");
        assert_eq!(
            result,
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
    }

    #[test]
    fn sha256_single_byte() {
        let result = sha256_hex(b"a");
        assert_eq!(
            result,
            "ca978112ca1bbdcafac231b39a23dc4da786eff8147c4e72b9807785afee48bb"
        );
    }

    // ---- Password hashing tests ----

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
    fn hash_password_different_salts() {
        let h1 = hash_password("mypassword", "salt1");
        let h2 = hash_password("mypassword", "salt2");
        assert_ne!(h1, h2);
    }

    #[test]
    fn hash_password_different_passwords() {
        let h1 = hash_password("password1", "samesalt");
        let h2 = hash_password("password2", "samesalt");
        assert_ne!(h1, h2);
    }

    #[test]
    fn hash_password_empty_password() {
        let hashed = hash_password("", "salt");
        assert!(hashed.starts_with("$sha256$salt$"));
        assert!(hashed.len() > "$sha256$salt$".len());
    }

    // ---- Verify password tests ----

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

    // ---- Constant-time comparison tests ----

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

    // ---- Password strength tests ----

    #[test]
    fn strength_strong_password() {
        let result = check_password_strength("P@ssw0rd!");
        assert!(result.ok);
        assert!(result.reasons.is_empty());
    }

    #[test]
    fn strength_too_short() {
        let result = check_password_strength("Ab1!");
        assert!(!result.ok);
        assert!(result.reasons.iter().any(|r| r.contains("too short")));
    }

    #[test]
    fn strength_missing_uppercase() {
        let result = check_password_strength("p@ssw0rd!");
        assert!(!result.ok);
        assert!(result.reasons.iter().any(|r| r.contains("uppercase")));
    }

    #[test]
    fn strength_missing_lowercase() {
        let result = check_password_strength("P@SSW0RD!");
        assert!(!result.ok);
        assert!(result.reasons.iter().any(|r| r.contains("lowercase")));
    }

    #[test]
    fn strength_missing_digit() {
        let result = check_password_strength("P@ssword!");
        assert!(!result.ok);
        assert!(result.reasons.iter().any(|r| r.contains("digit")));
    }

    #[test]
    fn strength_missing_special() {
        let result = check_password_strength("Passw0rds");
        assert!(!result.ok);
        assert!(result.reasons.iter().any(|r| r.contains("special")));
    }

    #[test]
    fn strength_common_pattern_password() {
        let result = check_password_strength("Password1!");
        assert!(!result.ok);
        assert!(result.reasons.iter().any(|r| r.contains("common pattern")));
    }

    #[test]
    fn strength_common_pattern_123456() {
        let result = check_password_strength("A!123456bcde");
        assert!(!result.ok);
        assert!(result.reasons.iter().any(|r| r.contains("common pattern")));
    }

    #[test]
    fn strength_all_same_char() {
        let result = check_password_strength("AAAAAAAA");
        assert!(!result.ok);
        assert!(result.reasons.iter().any(|r| r.contains("same character")));
    }

    #[test]
    fn strength_empty_password() {
        let result = check_password_strength("");
        assert!(!result.ok);
        assert!(result.reasons.iter().any(|r| r.contains("too short")));
    }

    // ---- Shadow entry tests ----

    #[test]
    fn shadow_parse_full_line() {
        let entry = ShadowEntry::parse(
            "alice:$sha256$salt$hash:19500:0:99999:7:30:20000:"
        ).expect("should parse");
        assert_eq!(entry.username, "alice");
        assert_eq!(entry.hash, "$sha256$salt$hash");
        assert_eq!(entry.last_changed, 19500);
        assert_eq!(entry.min_days, 0);
        assert_eq!(entry.max_days, 99999);
        assert_eq!(entry.warn_days, 7);
        assert_eq!(entry.inactive_days, 30);
        assert_eq!(entry.expire_date, 20000);
    }

    #[test]
    fn shadow_parse_minimal() {
        let entry = ShadowEntry::parse("bob:!:19000:0:99999:7:::");
        assert!(entry.is_some());
        let entry = entry.unwrap();
        assert_eq!(entry.username, "bob");
        assert!(entry.is_locked());
    }

    #[test]
    fn shadow_parse_too_short() {
        assert!(ShadowEntry::parse("user:hash:123").is_none());
    }

    #[test]
    fn shadow_round_trip() {
        let original = ShadowEntry {
            username: "testuser".to_string(),
            hash: "$sha256$salt$abcdef".to_string(),
            last_changed: 19500,
            min_days: 1,
            max_days: 90,
            warn_days: 14,
            inactive_days: 30,
            expire_date: 20000,
        };
        let line = original.to_line();
        let parsed = ShadowEntry::parse(&line).expect("should parse round-trip");
        assert_eq!(parsed.username, original.username);
        assert_eq!(parsed.hash, original.hash);
        assert_eq!(parsed.last_changed, original.last_changed);
        assert_eq!(parsed.min_days, original.min_days);
        assert_eq!(parsed.max_days, original.max_days);
        assert_eq!(parsed.warn_days, original.warn_days);
        assert_eq!(parsed.inactive_days, original.inactive_days);
        assert_eq!(parsed.expire_date, original.expire_date);
    }

    #[test]
    fn shadow_to_line_negative_inactive() {
        let entry = ShadowEntry {
            username: "user".to_string(),
            hash: "hash".to_string(),
            last_changed: 100,
            min_days: 0,
            max_days: 99999,
            warn_days: 7,
            inactive_days: -1,
            expire_date: -1,
        };
        let line = entry.to_line();
        // Negative values should be serialized as empty fields.
        assert!(line.contains("::"));
    }

    #[test]
    fn shadow_is_locked() {
        let mut entry = ShadowEntry::new("test");
        entry.hash = "!$sha256$salt$hash".to_string();
        assert!(entry.is_locked());
    }

    #[test]
    fn shadow_not_locked() {
        let mut entry = ShadowEntry::new("test");
        entry.hash = "$sha256$salt$hash".to_string();
        assert!(!entry.is_locked());
    }

    #[test]
    fn shadow_is_passwordless() {
        let mut entry = ShadowEntry::new("test");
        entry.hash = String::new();
        assert!(entry.is_passwordless());
    }

    #[test]
    fn shadow_not_passwordless() {
        let mut entry = ShadowEntry::new("test");
        entry.hash = "$sha256$salt$hash".to_string();
        assert!(!entry.is_passwordless());
    }

    #[test]
    fn shadow_status_char_locked() {
        let mut entry = ShadowEntry::new("test");
        entry.hash = "!something".to_string();
        assert_eq!(entry.status_char(), "L");
    }

    #[test]
    fn shadow_status_char_no_password() {
        let mut entry = ShadowEntry::new("test");
        entry.hash = String::new();
        assert_eq!(entry.status_char(), "NP");
    }

    #[test]
    fn shadow_status_char_has_password() {
        let mut entry = ShadowEntry::new("test");
        entry.hash = "$sha256$salt$hash".to_string();
        assert_eq!(entry.status_char(), "P");
    }

    // ---- Passwd entry tests ----

    #[test]
    fn passwd_parse_valid() {
        let entry = PasswdEntry::parse("alice:x:1000:1000:Alice:/home/alice:/bin/sh");
        assert!(entry.is_some());
        let entry = entry.unwrap();
        assert_eq!(entry.username, "alice");
        assert_eq!(entry.uid, 1000);
    }

    #[test]
    fn passwd_parse_root() {
        let entry = PasswdEntry::parse("root:x:0:0:root:/root:/bin/sh");
        assert!(entry.is_some());
        let entry = entry.unwrap();
        assert_eq!(entry.username, "root");
        assert_eq!(entry.uid, 0);
    }

    #[test]
    fn passwd_parse_too_short() {
        assert!(PasswdEntry::parse("user:x").is_none());
    }

    #[test]
    fn passwd_parse_bad_uid() {
        assert!(PasswdEntry::parse("user:x:notanumber:0::/home:/bin/sh").is_none());
    }

    // ---- Argument parsing tests ----

    #[test]
    fn args_default_change_password() {
        let args = vec!["passwd".to_string()];
        let parsed = parse_args(&args).unwrap();
        assert!(matches!(parsed.action, Action::ChangePassword));
        assert!(parsed.target_user.is_none());
    }

    #[test]
    fn args_change_password_for_user() {
        let args = vec!["passwd".to_string(), "alice".to_string()];
        let parsed = parse_args(&args).unwrap();
        assert!(matches!(parsed.action, Action::ChangePassword));
        assert_eq!(parsed.target_user.as_deref(), Some("alice"));
    }

    #[test]
    fn args_lock() {
        let args = vec!["passwd".to_string(), "-l".to_string(), "bob".to_string()];
        let parsed = parse_args(&args).unwrap();
        assert!(matches!(parsed.action, Action::Lock));
        assert_eq!(parsed.target_user.as_deref(), Some("bob"));
    }

    #[test]
    fn args_unlock() {
        let args = vec!["passwd".to_string(), "-u".to_string(), "bob".to_string()];
        let parsed = parse_args(&args).unwrap();
        assert!(matches!(parsed.action, Action::Unlock));
    }

    #[test]
    fn args_delete() {
        let args = vec!["passwd".to_string(), "-d".to_string(), "bob".to_string()];
        let parsed = parse_args(&args).unwrap();
        assert!(matches!(parsed.action, Action::DeletePassword));
    }

    #[test]
    fn args_status() {
        let args = vec!["passwd".to_string(), "-S".to_string(), "bob".to_string()];
        let parsed = parse_args(&args).unwrap();
        assert!(matches!(parsed.action, Action::ShowStatus));
    }

    #[test]
    fn args_expire() {
        let args = vec!["passwd".to_string(), "-e".to_string(), "bob".to_string()];
        let parsed = parse_args(&args).unwrap();
        assert!(matches!(parsed.action, Action::Expire));
    }

    #[test]
    fn args_min_days() {
        let args = vec![
            "passwd".to_string(), "-n".to_string(), "5".to_string(), "bob".to_string(),
        ];
        let parsed = parse_args(&args).unwrap();
        assert!(matches!(parsed.action, Action::SetMinDays(5)));
    }

    #[test]
    fn args_max_days() {
        let args = vec![
            "passwd".to_string(), "-x".to_string(), "90".to_string(), "bob".to_string(),
        ];
        let parsed = parse_args(&args).unwrap();
        assert!(matches!(parsed.action, Action::SetMaxDays(90)));
    }

    #[test]
    fn args_warn_days() {
        let args = vec![
            "passwd".to_string(), "-w".to_string(), "14".to_string(), "bob".to_string(),
        ];
        let parsed = parse_args(&args).unwrap();
        assert!(matches!(parsed.action, Action::SetWarnDays(14)));
    }

    #[test]
    fn args_inactive_days() {
        let args = vec![
            "passwd".to_string(), "-i".to_string(), "30".to_string(), "bob".to_string(),
        ];
        let parsed = parse_args(&args).unwrap();
        assert!(matches!(parsed.action, Action::SetInactiveDays(30)));
    }

    #[test]
    fn args_unknown_option() {
        let args = vec!["passwd".to_string(), "-Z".to_string()];
        assert!(parse_args(&args).is_err());
    }

    #[test]
    fn args_missing_days_value() {
        let args = vec!["passwd".to_string(), "-n".to_string()];
        assert!(parse_args(&args).is_err());
    }

    #[test]
    fn args_invalid_days_value() {
        let args = vec!["passwd".to_string(), "-n".to_string(), "abc".to_string()];
        assert!(parse_args(&args).is_err());
    }

    #[test]
    fn args_duplicate_username() {
        let args = vec![
            "passwd".to_string(), "alice".to_string(), "bob".to_string(),
        ];
        assert!(parse_args(&args).is_err());
    }

    // ---- Date conversion tests ----

    #[test]
    fn days_to_date_epoch() {
        assert_eq!(days_to_date_string(0), "1970-01-01");
    }

    #[test]
    fn days_to_date_known() {
        // 2024-01-01 is day 19723 since epoch.
        assert_eq!(days_to_date_string(19723), "2024-01-01");
    }

    #[test]
    fn days_to_date_negative() {
        assert_eq!(days_to_date_string(-5), "1970-01-01");
    }

    #[test]
    fn days_to_date_leap_year() {
        // 2000-03-01 is day 11017 since epoch.
        assert_eq!(days_to_date_string(11017), "2000-03-01");
    }

    // ---- Leap year tests ----

    #[test]
    fn leap_year_2000() {
        assert!(is_leap_year(2000));
    }

    #[test]
    fn leap_year_2024() {
        assert!(is_leap_year(2024));
    }

    #[test]
    fn not_leap_year_1900() {
        assert!(!is_leap_year(1900));
    }

    #[test]
    fn not_leap_year_2023() {
        assert!(!is_leap_year(2023));
    }

    // ---- Shadow new() defaults test ----

    #[test]
    fn shadow_new_defaults() {
        let entry = ShadowEntry::new("newuser");
        assert_eq!(entry.username, "newuser");
        assert_eq!(entry.hash, "!");
        assert_eq!(entry.min_days, 0);
        assert_eq!(entry.max_days, 99999);
        assert_eq!(entry.warn_days, 7);
        assert_eq!(entry.inactive_days, -1);
        assert_eq!(entry.expire_date, -1);
    }

    // ---- find_or_create_shadow tests ----

    #[test]
    fn find_or_create_existing() {
        let mut entries = vec![
            ShadowEntry::new("alice"),
            ShadowEntry::new("bob"),
        ];
        let idx = find_or_create_shadow(&mut entries, "alice");
        assert_eq!(idx, 0);
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn find_or_create_new() {
        let mut entries = vec![ShadowEntry::new("alice")];
        let idx = find_or_create_shadow(&mut entries, "charlie");
        assert_eq!(idx, 1);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[1].username, "charlie");
    }

    // ---- Salt generation test ----

    #[test]
    fn salt_is_hex_and_right_length() {
        let salt = generate_salt();
        assert_eq!(salt.len(), 32); // 16 bytes * 2 hex chars each
        assert!(salt.chars().all(|c| c.is_ascii_hexdigit()));
    }

    // ---- Integration: hash + verify round-trip ----

    #[test]
    fn hash_verify_round_trip() {
        let password = "S3cur3!Pass";
        let salt = "0123456789abcdef";
        let hashed = hash_password(password, salt);
        assert!(verify_password(password, &hashed));
        assert!(!verify_password("wrong", &hashed));
    }
}
