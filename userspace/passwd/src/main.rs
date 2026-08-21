//! Slate OS Password Management Utility
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
// SHA-256 implementation — deleted
// ============================================================================
//
// This file used to carry a full, genuine SHA-256 (round constants, FIPS
// vectors and all) and use it to write `/etc/shadow` entries in a format it
// invented: `$sha256$<salt>$<64 hex digits>`.  Everything about it was
// right except the two things that mattered.
//
// It was the wrong *format*: `$5$` and `$6$` are the crypt(3) identifiers
// for SHA-crypt, and a reader that follows the standard — a real libc, or
// `posix/src/crypt.rs` — parses `$sha256$` as an unknown method and refuses
// it.  `login`, which read the same file, refused it too, so a password set
// with `passwd` could not be used to log in.
//
// And it was the wrong *construction*: one pass of SHA-256 over
// `salt$password` has no work factor.  Every real crypt(3) scheme iterates
// thousands of rounds so that testing a guess costs the attacker what one
// login costs the user; a single pass costs the attacker nothing.
//
// Both are fixed by not implementing any of it here.  See
// `requests/c-b-passwd-and-login-disagree-about-etc-shadow.md`.

// ============================================================================
// Password hashing and salt generation
// ============================================================================

/// The method `passwd` selects for a new password.
///
/// SHA-512 because that is what the shadow suite defaults to, and because
/// the three tools that share `/etc/shadow` must agree: an entry written
/// here is read by `login` and rewritten by `chpasswd`, and the cheapest
/// way to keep them consistent is for all of them to name the same default.
const NEW_PASSWORD_METHOD: posix::crypt::Method = posix::crypt::Method::Sha512;

/// Hash `password` under a fresh setting built from `salt`.
///
/// Returns `None` for a salt `crypt` cannot carry verbatim — one that is
/// empty, too long for the method, or contains a character outside the
/// crypt base-64 alphabet.  [`generate_salt`] cannot produce such a salt,
/// so in practice this is `None` only if a caller invents one; it is a
/// `Result`-shaped answer rather than a silent truncation because a
/// truncated salt would mean the entry stored is not the entry asked for.
fn hash_password(password: &str, salt: &str) -> Option<String> {
    let mut setting_buf = posix::crypt::buf();
    let setting =
        posix::crypt::setting_into(NEW_PASSWORD_METHOD, salt.as_bytes(), &mut setting_buf)?;
    let mut hash_buf = posix::crypt::buf();
    Some(
        posix::crypt::hash_into(password.as_bytes(), setting.as_bytes(), &mut hash_buf)?
            .to_string(),
    )
}

/// Encode random bytes as a salt in the crypt base-64 alphabet.
///
/// `& 0x3f` is an unbiased reduction here and not the usual modulo mistake:
/// 256 is exactly four times 64, so every alphabet character is the image
/// of exactly four byte values.
fn encode_salt(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"./0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
    bytes
        .iter()
        .map(|b| char::from(ALPHABET[usize::from(*b & 0x3f)]))
        .collect()
}

/// Generate a random salt for a new password, or `None` if the system has no
/// randomness to draw it from.
///
/// The output is characters of the crypt base-64 alphabet, not hex, and is
/// exactly [`posix::crypt::Method::salt_max`] of them.  Both details matter:
/// SHA-crypt truncates a longer salt when hashing but stores what it was
/// given, so a 32-character hex salt — which is what this function used to
/// return — produces an entry that can never verify against itself.
///
/// **There is deliberately no fallback.**  An earlier version fell back to a
/// day-number generator, which is a salt in shape only: the day is public, so
/// the whole salt follows from it, every account given a password on the same
/// day shares it, and one precomputed table covers them all — the exact
/// property a salt exists to deny.  A password file that cannot be attacked
/// is worth more than a `passwd` that always succeeds, so this refuses and
/// says why, as `chpasswd` already did.  Tests pass their own salt to
/// [`hash_password`] rather than being served by a weakened production path.
fn generate_salt() -> Option<String> {
    let len = NEW_PASSWORD_METHOD.salt_max();
    let data = fs::read("/dev/urandom").ok()?;
    Some(encode_salt(data.get(..len)?))
}

/// Verify a password against a stored `/etc/shadow` entry.
///
/// Deliberately holds no knowledge of the format: the stored entry is
/// itself the setting, so `crypt` reads the method, the rounds and the salt
/// back out of it.  The previous version parsed `$sha256$<salt>$<hash>` by
/// hand and returned `false` for everything else, which is how it came to
/// disagree with `login` about the same file.
fn verify_password(password: &str, stored_hash: &str) -> bool {
    posix::crypt::verify(password.as_bytes(), stored_hash.as_bytes())
}

// The constant-time comparison that stood here went with the hand-written
// format parsing that was its only caller.  It now lives inside
// `posix::crypt::verify`, next to the value it compares against, where a
// caller cannot reach past it and compare something else.

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
/// On Slate OS, we disable echo via ioctl on /dev/tty.
/// Falls back to normal line read if terminal control is unavailable.
fn read_password_no_echo(prompt: &str) -> Result<String, String> {
    eprint!("{prompt}");
    let _ = io::stderr().flush();

    // Attempt to disable echo. On Slate OS this would use termios ioctls.
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
            // No `&& !entry.is_locked()` here: a locked account was already
            // refused above, so the conjunct could never be false, and keeping
            // it read as though a locked account reached this prompt and was
            // waved past it -- the opposite of what happens.  Locking is
            // decided in exactly one place, and this is not it.
            if !entry.hash.is_empty() {
                let old_pw = match read_password_no_echo("Current password: ") {
                    Ok(p) => p,
                    Err(e) => {
                        eprintln!("passwd: {e}");
                        return 1;
                    }
                };
                if !verify_password(&old_pw, &entry.hash) {
                    // An entry in a format nothing can recompute is not a
                    // wrong password, and telling the user "authentication
                    // failure" would send them away retyping a password
                    // that was never going to work.  The remedy is root
                    // setting a new one, so say so.  Only the account's own
                    // owner reaches this branch — root skips the old-password
                    // check entirely — so it discloses nothing.
                    if posix::crypt::stored_method(entry.hash.as_bytes()).is_none() {
                        eprintln!(
                            "passwd: the stored password for `{target}' is not in a format \
                             this system can verify, so it cannot be confirmed; ask an \
                             administrator to run `passwd {target}' as root"
                        );
                        return 1;
                    }
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

    // Hash and store.  A salt this build cannot carry verbatim would mean
    // writing an entry that is not the one asked for, so it aborts rather
    // than storing a truncated one — and since `generate_salt` draws from a
    // fixed alphabet at the method's own maximum length, reaching this is a
    // bug here, not bad input.
    let Some(salt) = generate_salt() else {
        eprintln!(
            "passwd: cannot read `/dev/urandom', so there is no random salt to \
             store this password with; refusing to write a password without one"
        );
        return 1;
    };
    let Some(hashed) = hash_password(&new_pw, &salt) else {
        eprintln!(
            "passwd: internal error: generated salt is not usable with {NEW_PASSWORD_METHOD:?}"
        );
        return 1;
    };

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

    eprintln!(
        "passwd: minimum password age for '{}' set to {} day(s)",
        target, days
    );
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

    eprintln!(
        "passwd: maximum password age for '{}' set to {} day(s)",
        target, days
    );
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
    (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400)
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
    let changing_own =
        parsed.target_user.is_none() || current_username().as_deref() == Some(target.as_str());

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

    // ---- Password hashing tests ----
    //
    // The SHA-256 known-answer tests that stood here went with the SHA-256
    // implementation they checked. Their vectors were correct and they
    // passed; what they could not tell anyone was that the correct digest
    // was being put into an invented format, under no key-stretching, in a
    // file two other programs also wrote. The tests below check that
    // instead, because that is where this file was actually wrong.

    /// A salt `crypt` can carry verbatim, in the alphabet `generate_salt`
    /// draws from.
    const SALT: &str = "abcdef0123456789";

    /// The entry written must be one a standard reader recognises: the
    /// `$6$` identifier, the salt as given, and an 86-character SHA-512
    /// crypt hash. The old format, `$sha256$<salt>$<64 hex>`, satisfied
    /// none of that, which is why `login` could not read it.
    #[test]
    fn hash_password_writes_a_standard_crypt_entry() {
        let hashed = hash_password("test123", SALT).expect("hash");
        assert!(hashed.starts_with("$6$abcdef0123456789$"), "{hashed}");
        assert_eq!(
            posix::crypt::stored_method(hashed.as_bytes()),
            Some(posix::crypt::Method::Sha512),
            "{hashed}"
        );
        assert!(!hashed.contains("$sha256$"), "{hashed}");
    }

    #[test]
    fn hash_password_deterministic() {
        let h1 = hash_password("mypassword", SALT).expect("hash");
        let h2 = hash_password("mypassword", SALT).expect("hash");
        assert_eq!(h1, h2);
    }

    #[test]
    fn hash_password_different_salts() {
        let h1 = hash_password("mypassword", "salt1").expect("hash");
        let h2 = hash_password("mypassword", "salt2").expect("hash");
        assert_ne!(h1, h2);
    }

    #[test]
    fn hash_password_different_passwords() {
        let h1 = hash_password("password1", "samesalt").expect("hash");
        let h2 = hash_password("password2", "samesalt").expect("hash");
        assert_ne!(h1, h2);
    }

    #[test]
    fn hash_password_empty_password() {
        let hashed = hash_password("", "salt").expect("hash");
        assert!(hashed.starts_with("$6$salt$"), "{hashed}");
        assert!(verify_password("", &hashed));
        assert!(!verify_password("x", &hashed));
    }

    /// A salt the format cannot carry is refused rather than truncated or
    /// silently reinterpreted: `$` would end the salt early, so the entry
    /// stored would not be the entry asked for.
    #[test]
    fn hash_password_refuses_a_salt_it_cannot_store() {
        assert_eq!(hash_password("pw", ""), None);
        assert_eq!(hash_password("pw", "has$dollar"), None);
        assert_eq!(hash_password("pw", "has space"), None);
        // 17 characters, one past SHA-crypt's maximum.
        assert_eq!(hash_password("pw", "abcdefghijklmnopq"), None);
    }

    /// The salt must be usable as-is by the method `passwd` writes with.
    /// It used to be 32 hex characters — twice SHA-crypt's maximum — which
    /// hashing truncates while storing what it was given, producing an entry
    /// that cannot verify against itself.
    ///
    /// Driven through [`encode_salt`] rather than through [`generate_salt`],
    /// because the development host has no `/dev/urandom` and, more to the
    /// point, a randomness source cannot be asked to produce its own worst
    /// case.  Every one of the 256 byte values is covered here; the hashing
    /// round-trip below needs only one salt, since the property that could
    /// break is the *encoding*, not the hash.
    #[test]
    fn every_byte_encodes_to_a_character_crypt_can_carry() {
        let len = NEW_PASSWORD_METHOD.salt_max();
        for start in 0..=u8::MAX {
            let bytes: Vec<u8> = (0..len).map(|i| start.wrapping_add(i as u8)).collect();
            let salt = encode_salt(&bytes);
            assert_eq!(salt.len(), len, "{salt}");
            assert!(
                salt.bytes()
                    .all(|b| b == b'.' || b == b'/' || b.is_ascii_alphanumeric()),
                "{salt}"
            );
        }
    }

    /// An encoded salt of the length `passwd` generates is stored verbatim and
    /// verifies against itself — the property the old 32-hex-character salt
    /// broke.
    #[test]
    fn an_encoded_salt_round_trips_through_the_stored_entry() {
        let len = NEW_PASSWORD_METHOD.salt_max();
        let salt = encode_salt(&(0..len).map(|i| i as u8).collect::<Vec<_>>());
        let hashed = hash_password("correct horse", &salt).expect("hash");
        assert!(hashed.starts_with(&format!("$6${salt}$")), "{hashed}");
        assert!(verify_password("correct horse", &hashed));
    }

    /// `passwd` refuses rather than substituting a predictable salt when the
    /// system has no randomness.  On a host with `/dev/urandom` this checks
    /// the salt it produced; on one without — which includes the development
    /// host — it checks that the answer is `None` and not a weak salt.
    #[test]
    fn generate_salt_either_draws_from_urandom_or_refuses() {
        match generate_salt() {
            Some(salt) => {
                assert_eq!(salt.len(), NEW_PASSWORD_METHOD.salt_max(), "{salt}");
                assert!(hash_password("pw", &salt).is_some(), "{salt}");
            }
            None => assert!(
                fs::read("/dev/urandom")
                    .map(|d| d.len() < NEW_PASSWORD_METHOD.salt_max())
                    .unwrap_or(true),
                "refused a salt despite /dev/urandom being readable"
            ),
        }
    }

    // ---- Verify password tests ----

    #[test]
    fn verify_correct_password() {
        let hashed = hash_password("correct_horse", SALT).expect("hash");
        assert!(verify_password("correct_horse", &hashed));
    }

    #[test]
    fn verify_wrong_password() {
        let hashed = hash_password("correct_horse", SALT).expect("hash");
        assert!(!verify_password("wrong_horse", &hashed));
    }

    #[test]
    fn verify_empty_hash() {
        assert!(!verify_password("anything", ""));
        assert!(!verify_password("", ""));
    }

    #[test]
    fn verify_malformed_hash() {
        assert!(!verify_password("test", "$sha256$noseparator"));
        assert!(!verify_password("test", "$6$noseparator"));
        assert!(!verify_password("test", "not a hash at all"));
        // No cleartext path: an entry that is not a hash matches nothing,
        // including itself.
        assert!(!verify_password("secret", "secret"));
    }

    /// A published SHA-crypt vector, checked through the function that
    /// verifies `/etc/shadow`. This is the test the old code could not
    /// have: its format had no specification and so no known answer.
    #[test]
    fn verify_accepts_a_published_vector() {
        const VECTOR: &str = "$6$saltstring$svn8UoSVapNtMuq1ukKS4tPQd8iKwSMHWjl/O817G3uBnIFNjnQJuesI68u4OTLiBFdcbYEdFCoEOfaS35inz1";
        assert!(verify_password("Hello world!", VECTOR));
        assert!(!verify_password("Hello world", VECTOR));
    }

    /// The entries this tree wrote before `passwd` called `crypt` — its own
    /// `$sha256$`, and `chpasswd`'s 64 hex digits mislabelled `$5$` — can
    /// never verify, and are recognisable as such by shape.
    #[test]
    fn the_obsolete_formats_are_recognisable_and_unverifiable() {
        let digest = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        for prefix in ["$sha256$", "$5$", "$6$", "$1$"] {
            let stored = format!("{prefix}{SALT}${digest}");
            assert_eq!(
                posix::crypt::stored_method(stored.as_bytes()),
                None,
                "{stored}"
            );
            assert!(!verify_password("correct horse", &stored), "{stored}");
        }
    }

    /// A locked entry is unverifiable **even with the correct password**.
    ///
    /// `cmd_change_password` refuses locked accounts in one place, up front.
    /// The old-password gate below it used to carry a second `!is_locked()`
    /// check that was already unreachable — and that duplicate failed *open*:
    /// had the up-front refusal ever been removed, the gate would have been
    /// skipped entirely and a locked account's password changed without the
    /// old one. With the duplicate gone, that path instead ends here, at a
    /// stored entry whose `!` prefix leaves it with no recomputable method.
    /// This test is what makes removing the duplicate safe, so it asserts the
    /// property directly rather than trusting the prefix to look wrong.
    #[test]
    fn a_locked_entry_verifies_against_nothing_not_even_the_right_password() {
        let hashed = hash_password("correct horse", SALT).expect("hash");
        assert!(verify_password("correct horse", &hashed));

        for locked in [
            format!("!{hashed}"),
            format!("!!{hashed}"),
            format!("*{hashed}"),
        ] {
            assert_eq!(
                posix::crypt::stored_method(locked.as_bytes()),
                None,
                "{locked}"
            );
            assert!(!verify_password("correct horse", &locked), "{locked}");
            assert!(!verify_password("", &locked), "{locked}");
        }
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
        let entry = ShadowEntry::parse("alice:$sha256$salt$hash:19500:0:99999:7:30:20000:")
            .expect("should parse");
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
            "passwd".to_string(),
            "-n".to_string(),
            "5".to_string(),
            "bob".to_string(),
        ];
        let parsed = parse_args(&args).unwrap();
        assert!(matches!(parsed.action, Action::SetMinDays(5)));
    }

    #[test]
    fn args_max_days() {
        let args = vec![
            "passwd".to_string(),
            "-x".to_string(),
            "90".to_string(),
            "bob".to_string(),
        ];
        let parsed = parse_args(&args).unwrap();
        assert!(matches!(parsed.action, Action::SetMaxDays(90)));
    }

    #[test]
    fn args_warn_days() {
        let args = vec![
            "passwd".to_string(),
            "-w".to_string(),
            "14".to_string(),
            "bob".to_string(),
        ];
        let parsed = parse_args(&args).unwrap();
        assert!(matches!(parsed.action, Action::SetWarnDays(14)));
    }

    #[test]
    fn args_inactive_days() {
        let args = vec![
            "passwd".to_string(),
            "-i".to_string(),
            "30".to_string(),
            "bob".to_string(),
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
        let args = vec!["passwd".to_string(), "alice".to_string(), "bob".to_string()];
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
        let mut entries = vec![ShadowEntry::new("alice"), ShadowEntry::new("bob")];
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

    // (The salt's shape is checked by
    // `generated_salt_is_usable_with_the_method_passwd_writes` above, which
    // asserts the property that matters — that the salt can be stored
    // verbatim by the method in use — rather than a length and an alphabet
    // chosen independently of it. The old test asserted 32 hex characters,
    // which was internally consistent and twice what SHA-crypt can carry.)

    // ---- Integration: hash + verify round-trip ----

    #[test]
    fn hash_verify_round_trip() {
        let password = "S3cur3!Pass";
        let salt = "0123456789abcdef";
        let hashed = hash_password(password, salt).expect("hash");
        assert!(verify_password(password, &hashed));
        assert!(!verify_password("wrong", &hashed));
    }
}
