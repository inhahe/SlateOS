// Slate OS login — user login program
//
// Authenticates users and starts their login session. Called by getty(8)
// after a username is entered, or directly for console login.
//
// Usage:
//   login [-f] [-h hostname] [-p] [--] [username]
//
// Features:
//   - Password authentication via /etc/shadow
//   - Login session setup (utmp, lastlog, motd, mail check)
//   - Environment initialization
//   - Shell spawning
//   - Failed login tracking and lockout
//   - PAM-like authentication flow (simplified)

#![cfg_attr(not(test), no_main)]
// Tracked-but-not-yet-wired fields and constants are kept to document the
// intended interface as the login implementation grows (PAM/shadow have
// many fields that the current minimal flow doesn't read yet).
#![allow(dead_code)]

use std::collections::HashMap;
use std::env;
use std::io::{self, BufRead, Write};
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const MAX_LOGIN_ATTEMPTS: u32 = 5;
const LOGIN_TIMEOUT_SECS: u64 = 60;
const DEFAULT_PATH: &str = "/usr/local/bin:/usr/bin:/bin";
const DEFAULT_ROOT_PATH: &str = "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";
const MOTD_FILE: &str = "/etc/motd";
const NOLOGIN_FILE: &str = "/etc/nologin";
const SECURETTY_FILE: &str = "/etc/securetty";
const PASSWD_FILE: &str = "/etc/passwd";
const SHADOW_FILE: &str = "/etc/shadow";
const LASTLOG_FILE: &str = "/var/log/lastlog";
const FAILLOG_FILE: &str = "/var/log/faillog";
const MAIL_DIR: &str = "/var/mail";
const ISSUE_FILE: &str = "/etc/issue";
const HUSHLOGIN_FILE: &str = ".hushlogin";

// ---------------------------------------------------------------------------
// Error types
// ---------------------------------------------------------------------------

#[derive(Debug)]
enum LoginError {
    AuthFailed(String),
    AccountLocked(String),
    NoLogin(String),
    InvalidUser(String),
    SystemError(String),
    Timeout,
}

impl std::fmt::Display for LoginError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AuthFailed(msg) => write!(f, "Authentication failure: {msg}"),
            Self::AccountLocked(msg) => write!(f, "Account locked: {msg}"),
            Self::NoLogin(msg) => write!(f, "{msg}"),
            Self::InvalidUser(msg) => write!(f, "Invalid user: {msg}"),
            Self::SystemError(msg) => write!(f, "System error: {msg}"),
            Self::Timeout => write!(f, "Login timed out"),
        }
    }
}

// ---------------------------------------------------------------------------
// User/group database
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct PasswdEntry {
    username: String,
    uid: u32,
    gid: u32,
    gecos: String,
    home_dir: PathBuf,
    shell: PathBuf,
}

#[derive(Debug, Clone)]
struct ShadowEntry {
    username: String,
    password_hash: String,
    last_changed: i64,
    min_days: i64,
    max_days: i64,
    warn_days: i64,
    inactive_days: i64,
    expire_date: i64,
}

fn parse_passwd_entry(line: &str) -> Option<PasswdEntry> {
    let fields: Vec<&str> = line.split(':').collect();
    if fields.len() < 7 {
        return None;
    }

    Some(PasswdEntry {
        username: fields[0].to_string(),
        uid: fields[2].parse().ok()?,
        gid: fields[3].parse().ok()?,
        gecos: fields[4].to_string(),
        home_dir: PathBuf::from(fields[5]),
        shell: PathBuf::from(fields[6]),
    })
}

fn parse_shadow_entry(line: &str) -> Option<ShadowEntry> {
    let fields: Vec<&str> = line.split(':').collect();
    if fields.len() < 9 {
        return None;
    }

    Some(ShadowEntry {
        username: fields[0].to_string(),
        password_hash: fields[1].to_string(),
        last_changed: fields[2].parse().unwrap_or(-1),
        min_days: fields[3].parse().unwrap_or(-1),
        max_days: fields[4].parse().unwrap_or(-1),
        warn_days: fields[5].parse().unwrap_or(-1),
        inactive_days: fields[6].parse().unwrap_or(-1),
        expire_date: fields[7].parse().unwrap_or(-1),
    })
}

fn lookup_passwd(username: &str) -> Option<PasswdEntry> {
    let content = std::fs::read_to_string(PASSWD_FILE).ok()?;
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(entry) = parse_passwd_entry(line)
            && entry.username == username
        {
            return Some(entry);
        }
    }
    None
}

fn lookup_shadow(username: &str) -> Option<ShadowEntry> {
    let content = std::fs::read_to_string(SHADOW_FILE).ok()?;
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(entry) = parse_shadow_entry(line)
            && entry.username == username
        {
            return Some(entry);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Password verification
// ---------------------------------------------------------------------------
//
// All of it is `posix::crypt`, and deliberately none of it is here.  This
// file used to carry its own `simple_hash` — the SHA-256 initial vector
// followed by two arithmetic operations per byte, one pass, no iteration —
// and `passwd` wrote entries with a genuine SHA-256 under a `$sha256$`
// label.  The two produced strings of the same length and never the same
// contents, so `login` rejected the correct password for every account
// `passwd` had touched, and no test caught it because no test crossed the
// two tools.  See `requests/c-b-passwd-and-login-disagree-about-etc-shadow.md`.
//
// The lesson is not "we picked the wrong hash": it is that three programs
// sharing one file each implemented the format separately.  So this file no
// longer implements it at all.  `crypt::verify` takes the stored entry as
// the setting, which is what makes a stored hash self-describing, and the
// comparison it performs is the only one there is.

/// The outcome of checking a supplied password against a stored entry.
///
/// Four cases rather than a `bool`, because the caller must treat two of
/// them the same way toward the user (say "Login incorrect", learn nothing)
/// and differently toward the administrator: an entry nothing can verify is
/// a broken system, not a mistyped password, and saying so is the only way
/// anyone finds out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PasswordCheck {
    /// The supplied password reproduces the stored entry.
    Accepted,
    /// The entry is one we can recompute, and the password does not match.
    Rejected,
    /// The entry is a lock marker (`!`, `!!`, `*`, or a `!`-prefixed hash).
    /// The account was deliberately disabled; no password opens it.
    Locked,
    /// The entry is in no format this system can recompute, so no password
    /// can ever match it.  That includes `x` (a `passwd(5)` marker that has
    /// no meaning in `shadow(5)`), a hash whose field lengths do not match
    /// the method it names — which is how the entries this tree wrote before
    /// it called `crypt` are recognised — and anything else unparseable.
    Unusable,
}

/// Check `password` against a `shadow(5)` password field.
fn check_password(password: &str, hash: &str) -> PasswordCheck {
    // A leading `!` or `*` marks the account disabled.  `!` is conventionally
    // *prefixed* to an otherwise-valid hash so the password survives an
    // unlock, so this is a prefix test and not an equality test: `!$6$…`
    // must not fall through and be verified as if the `!` were salt.
    if hash.starts_with('!') || hash.starts_with('*') {
        return PasswordCheck::Locked;
    }

    // Traditional Unix passwordless account: the empty password, and only
    // the empty password, authenticates.
    if hash.is_empty() {
        return if password.is_empty() {
            PasswordCheck::Accepted
        } else {
            PasswordCheck::Rejected
        };
    }

    // Asked *before* verifying, so that an entry which can never verify is
    // reported as broken rather than counted as a wrong password.  There is
    // no cleartext fallback: an entry that is not a hash is not a password.
    if posix::crypt::stored_method(hash.as_bytes()).is_none() {
        return PasswordCheck::Unusable;
    }

    if posix::crypt::verify(password.as_bytes(), hash.as_bytes()) {
        PasswordCheck::Accepted
    } else {
        PasswordCheck::Rejected
    }
}

// ---------------------------------------------------------------------------
// Security checks
// ---------------------------------------------------------------------------

/// Check if nologin is in effect
fn check_nologin(uid: u32) -> Result<(), LoginError> {
    // Root can always log in
    if uid == 0 {
        return Ok(());
    }

    if let Ok(content) = std::fs::read_to_string(NOLOGIN_FILE) {
        let msg = if content.trim().is_empty() {
            "System is unavailable".to_string()
        } else {
            content.trim().to_string()
        };
        return Err(LoginError::NoLogin(msg));
    }

    // Also check /var/run/nologin
    if let Ok(content) = std::fs::read_to_string("/var/run/nologin") {
        let msg = if content.trim().is_empty() {
            "System is unavailable".to_string()
        } else {
            content.trim().to_string()
        };
        return Err(LoginError::NoLogin(msg));
    }

    Ok(())
}

/// Check if TTY is listed in /etc/securetty (for root login)
fn check_securetty(uid: u32, tty: &str) -> Result<(), LoginError> {
    if uid != 0 {
        return Ok(()); // Only applies to root
    }

    let content = match std::fs::read_to_string(SECURETTY_FILE) {
        Ok(c) => c,
        Err(_) => return Ok(()), // No securetty = all ttys allowed
    };

    let tty_short = tty.strip_prefix("/dev/").unwrap_or(tty);

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line == tty_short || line == tty {
            return Ok(());
        }
    }

    Err(LoginError::AuthFailed(
        "root login refused on this terminal".to_string(),
    ))
}

/// Check if account is expired
fn check_account_expired(shadow: &ShadowEntry) -> Result<(), LoginError> {
    if shadow.expire_date > 0 {
        // Would check against current time
        // For now, just check if it's set to a very old date
        if shadow.expire_date == 1 {
            return Err(LoginError::AccountLocked("account has expired".to_string()));
        }
    }

    // Check if password is locked
    if shadow.password_hash.starts_with('!') || shadow.password_hash.starts_with('*') {
        return Err(LoginError::AccountLocked("account is locked".to_string()));
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Session setup
// ---------------------------------------------------------------------------

/// Build the login environment
fn build_environment(user: &PasswdEntry, preserve_env: bool) -> HashMap<String, String> {
    let mut env_map = HashMap::new();

    if !preserve_env {
        // Start fresh
        env_map.insert("HOME".to_string(), user.home_dir.display().to_string());
        env_map.insert("SHELL".to_string(), user.shell.display().to_string());
        env_map.insert("USER".to_string(), user.username.clone());
        env_map.insert("LOGNAME".to_string(), user.username.clone());

        if user.uid == 0 {
            env_map.insert("PATH".to_string(), DEFAULT_ROOT_PATH.to_string());
        } else {
            env_map.insert("PATH".to_string(), DEFAULT_PATH.to_string());
        }

        // Preserve TERM if set
        if let Ok(term) = env::var("TERM") {
            env_map.insert("TERM".to_string(), term);
        } else {
            env_map.insert("TERM".to_string(), "linux".to_string());
        }
    } else {
        // Preserve current environment, just update user-specific vars
        for (key, val) in env::vars() {
            env_map.insert(key, val);
        }
        env_map.insert("HOME".to_string(), user.home_dir.display().to_string());
        env_map.insert("SHELL".to_string(), user.shell.display().to_string());
        env_map.insert("USER".to_string(), user.username.clone());
        env_map.insert("LOGNAME".to_string(), user.username.clone());
    }

    // Mail
    env_map.insert("MAIL".to_string(), format!("{MAIL_DIR}/{}", user.username));

    env_map
}

/// Display message of the day
fn display_motd(writer: &mut dyn Write) -> io::Result<()> {
    if let Ok(content) = std::fs::read_to_string(MOTD_FILE)
        && !content.is_empty()
    {
        write!(writer, "{content}")?;
    }
    Ok(())
}

/// Check for hushlogin
fn is_hushlogin(user: &PasswdEntry) -> bool {
    // Check user's home directory for .hushlogin
    let hush_path = user.home_dir.join(HUSHLOGIN_FILE);
    hush_path.exists()
}

/// Check for new mail
fn check_mail(writer: &mut dyn Write, username: &str) -> io::Result<()> {
    let mail_path = format!("{MAIL_DIR}/{username}");
    if let Ok(meta) = std::fs::metadata(&mail_path)
        && meta.len() > 0
    {
        writeln!(writer, "You have mail.")?;
    }
    Ok(())
}

/// Record login in lastlog
fn record_lastlog(username: &str, tty: &str, host: &str) {
    // Write a lastlog entry — in real system this would be a binary format
    let entry = format!("{username}:{tty}:{host}\n");
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(LASTLOG_FILE)
        .and_then(|mut f| f.write_all(entry.as_bytes()));
}

/// Record failed login attempt
fn record_faillog(username: &str, tty: &str) {
    let entry = format!("FAILED:{username}:{tty}\n");
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(FAILLOG_FILE)
        .and_then(|mut f| f.write_all(entry.as_bytes()));
}

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
struct Config {
    username: Option<String>,
    force_login: bool,        // -f: skip authentication
    hostname: Option<String>, // -h: remote host
    preserve_env: bool,       // -p: preserve environment
    show_help: bool,
    show_version: bool,
}

fn parse_args(args: &[String]) -> Result<Config, String> {
    let mut cfg = Config::default();
    let mut i = 1;
    let mut seen_dashdash = false;

    while i < args.len() {
        let arg = &args[i];

        if seen_dashdash {
            if cfg.username.is_none() {
                cfg.username = Some(arg.clone());
            }
            i += 1;
            continue;
        }

        match arg.as_str() {
            "--" => seen_dashdash = true,
            "-h" if i + 1 < args.len() => {
                i += 1;
                cfg.hostname = Some(args[i].clone());
            }
            "-f" => cfg.force_login = true,
            "-p" => cfg.preserve_env = true,
            "--help" => cfg.show_help = true,
            "--version" => cfg.show_version = true,
            other if other.starts_with('-') => {
                return Err(format!("unknown option: {other}"));
            }
            _ => {
                if cfg.username.is_none() {
                    cfg.username = Some(arg.clone());
                }
            }
        }
        i += 1;
    }

    Ok(cfg)
}

// ---------------------------------------------------------------------------
// Main login flow
// ---------------------------------------------------------------------------

fn do_login(
    cfg: &Config,
    reader: &mut dyn BufRead,
    writer: &mut dyn Write,
) -> Result<(PasswdEntry, HashMap<String, String>), LoginError> {
    let mut attempts = 0u32;

    loop {
        // Get username
        let username = if let Some(ref name) = cfg.username {
            name.clone()
        } else {
            write!(writer, "login: ").map_err(|e| LoginError::SystemError(e.to_string()))?;
            writer
                .flush()
                .map_err(|e| LoginError::SystemError(e.to_string()))?;

            let mut buf = String::new();
            reader
                .read_line(&mut buf)
                .map_err(|e| LoginError::SystemError(e.to_string()))?;
            let name = buf.trim().to_string();
            if name.is_empty() {
                continue;
            }
            name
        };

        // Look up user
        let user = match lookup_passwd(&username) {
            Some(u) => u,
            None => {
                attempts = attempts.saturating_add(1);
                // Delay to slow brute force - simulate reading password even for bad users
                if !cfg.force_login {
                    write!(writer, "Password: ")
                        .map_err(|e| LoginError::SystemError(e.to_string()))?;
                    writer
                        .flush()
                        .map_err(|e| LoginError::SystemError(e.to_string()))?;
                    let mut _discard = String::new();
                    let _ = reader.read_line(&mut _discard);
                }
                writeln!(writer, "Login incorrect")
                    .map_err(|e| LoginError::SystemError(e.to_string()))?;
                record_faillog(&username, "console");

                if attempts >= MAX_LOGIN_ATTEMPTS {
                    return Err(LoginError::AuthFailed(
                        "too many failed attempts".to_string(),
                    ));
                }
                if cfg.username.is_some() {
                    return Err(LoginError::InvalidUser(username));
                }
                continue;
            }
        };

        // Check nologin
        check_nologin(user.uid)?;

        // Check securetty for root
        let tty = env::var("TTY").unwrap_or_else(|_| "console".to_string());
        check_securetty(user.uid, &tty)?;

        // Check shadow entry
        if let Some(shadow) = lookup_shadow(&username) {
            check_account_expired(&shadow)?;

            // Authenticate (unless -f for forced login)
            if !cfg.force_login {
                write!(writer, "Password: ").map_err(|e| LoginError::SystemError(e.to_string()))?;
                writer
                    .flush()
                    .map_err(|e| LoginError::SystemError(e.to_string()))?;

                let mut password = String::new();
                reader
                    .read_line(&mut password)
                    .map_err(|e| LoginError::SystemError(e.to_string()))?;
                let password = password.trim_end_matches('\n').trim_end_matches('\r');

                let outcome = check_password(password, &shadow.password_hash);
                if outcome != PasswordCheck::Accepted {
                    attempts = attempts.saturating_add(1);
                    writeln!(writer, "Login incorrect")
                        .map_err(|e| LoginError::SystemError(e.to_string()))?;
                    record_faillog(&username, &tty);

                    // The user is told only "Login incorrect", whatever the
                    // reason — but an entry nothing can verify is a broken
                    // system rather than a wrong password, and if login says
                    // nothing about it the administrator's only symptom is a
                    // user who insists their password is right.  It leaks
                    // nothing exploitable: the account cannot be entered
                    // either way, and the remedy needs root.
                    if outcome == PasswordCheck::Unusable {
                        eprintln!(
                            "login: the password entry for `{username}' is not in a format \
                             this system can verify, so no password will be accepted for it; \
                             reset it with `passwd {username}' as root"
                        );
                    }

                    if attempts >= MAX_LOGIN_ATTEMPTS {
                        return Err(LoginError::AuthFailed(
                            "too many failed attempts".to_string(),
                        ));
                    }
                    if cfg.username.is_some() {
                        return Err(LoginError::AuthFailed("authentication failure".to_string()));
                    }
                    continue;
                }
            }
        } else if !cfg.force_login {
            // No shadow entry.  This branch used to prompt for a password,
            // discard it, and log the user in — an unconditional
            // authentication bypass for any account missing from
            // `/etc/shadow`, and for *every* account if the file itself was
            // absent or unreadable.  `PasswdEntry` does not even carry the
            // `passwd(5)` password column, so there was nothing here to
            // check against; the prompt was decoration.
            //
            // An account with no verifiable secret is an account that cannot
            // be authenticated, so it is not entered.  The prompt is still
            // shown and the answer still read, so that a missing entry takes
            // the same path — and the same time — as a wrong password, and
            // cannot be used to enumerate which users have shadow entries.
            write!(writer, "Password: ").map_err(|e| LoginError::SystemError(e.to_string()))?;
            writer
                .flush()
                .map_err(|e| LoginError::SystemError(e.to_string()))?;

            let mut password = String::new();
            reader
                .read_line(&mut password)
                .map_err(|e| LoginError::SystemError(e.to_string()))?;
            drop(password);

            attempts = attempts.saturating_add(1);
            writeln!(writer, "Login incorrect")
                .map_err(|e| LoginError::SystemError(e.to_string()))?;
            record_faillog(&username, &tty);
            eprintln!(
                "login: no `/etc/shadow' entry for `{username}', so there is no password \
                 to check; create one with `passwd {username}' as root"
            );

            if attempts >= MAX_LOGIN_ATTEMPTS {
                return Err(LoginError::AuthFailed(
                    "too many failed attempts".to_string(),
                ));
            }
            if cfg.username.is_some() {
                return Err(LoginError::AuthFailed("authentication failure".to_string()));
            }
            continue;
        }

        // Build environment
        let env_map = build_environment(&user, cfg.preserve_env);

        // Record successful login
        let host = cfg.hostname.as_deref().unwrap_or("localhost");
        record_lastlog(&username, &tty, host);

        // Display motd and mail check (unless hushlogin)
        if !is_hushlogin(&user) {
            let _ = display_motd(writer);
            let _ = check_mail(writer, &username);
        }

        return Ok((user, env_map));
    }
}

// ---------------------------------------------------------------------------
// Help / version
// ---------------------------------------------------------------------------

fn print_help() {
    println!("Usage: login [-f] [-h hostname] [-p] [--] [username]");
    println!();
    println!("Begin a session on the system.");
    println!();
    println!("Options:");
    println!("  -f             Skip authentication (pre-authenticated by getty)");
    println!("  -h <hostname>  Remote host for this login");
    println!("  -p             Preserve the environment (don't reset PATH, etc.)");
    println!("  --             End of options");
    println!("  --help         Show this help");
    println!("  --version      Show version");
}

fn print_version() {
    println!("login (Slate OS) 0.1.0");
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

#[cfg(not(test))]
#[unsafe(no_mangle)]
pub extern "C" fn main(_argc: i32, _argv: *const *const u8) -> i32 {
    let args: Vec<String> = env::args().collect();

    let cfg = match parse_args(&args) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("login: {e}");
            return 1;
        }
    };

    if cfg.show_help {
        print_help();
        return 0;
    }

    if cfg.show_version {
        print_version();
        return 0;
    }

    let stdin = io::stdin();
    let mut reader = stdin.lock();
    let stdout = io::stdout();
    let mut writer = stdout.lock();

    match do_login(&cfg, &mut reader, &mut writer) {
        Ok((user, env_map)) => {
            // In a real OS, we would:
            // 1. setuid/setgid to the user
            // 2. chdir to home directory
            // 3. exec the user's shell
            eprintln!(
                "login: would exec shell {} as user {} (uid={}, gid={})",
                user.shell.display(),
                user.username,
                user.uid,
                user.gid
            );
            eprintln!(
                "login: environment: HOME={}",
                env_map.get("HOME").unwrap_or(&String::new())
            );
            0
        }
        Err(e) => {
            eprintln!("login: {e}");
            1
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_parse_args_basic() {
        let args = vec!["login".to_string(), "testuser".to_string()];
        let cfg = parse_args(&args).unwrap();
        assert_eq!(cfg.username, Some("testuser".to_string()));
        assert!(!cfg.force_login);
    }

    #[test]
    fn test_parse_args_force() {
        let args = vec!["login".to_string(), "-f".to_string(), "root".to_string()];
        let cfg = parse_args(&args).unwrap();
        assert!(cfg.force_login);
        assert_eq!(cfg.username, Some("root".to_string()));
    }

    #[test]
    fn test_parse_args_host() {
        let args = vec![
            "login".to_string(),
            "-h".to_string(),
            "remote.host".to_string(),
            "user1".to_string(),
        ];
        let cfg = parse_args(&args).unwrap();
        assert_eq!(cfg.hostname, Some("remote.host".to_string()));
        assert_eq!(cfg.username, Some("user1".to_string()));
    }

    #[test]
    fn test_parse_args_preserve() {
        let args = vec!["login".to_string(), "-p".to_string(), "user1".to_string()];
        let cfg = parse_args(&args).unwrap();
        assert!(cfg.preserve_env);
    }

    #[test]
    fn test_parse_args_dashdash() {
        let args = vec!["login".to_string(), "--".to_string(), "-user".to_string()];
        let cfg = parse_args(&args).unwrap();
        assert_eq!(cfg.username, Some("-user".to_string()));
    }

    #[test]
    fn test_parse_args_no_username() {
        let args = vec!["login".to_string()];
        let cfg = parse_args(&args).unwrap();
        assert_eq!(cfg.username, None);
    }

    #[test]
    fn test_parse_args_help() {
        let args = vec!["login".to_string(), "--help".to_string()];
        let cfg = parse_args(&args).unwrap();
        assert!(cfg.show_help);
    }

    #[test]
    fn test_parse_args_version() {
        let args = vec!["login".to_string(), "--version".to_string()];
        let cfg = parse_args(&args).unwrap();
        assert!(cfg.show_version);
    }

    #[test]
    fn test_parse_passwd_entry() {
        let line = "root:x:0:0:root:/root:/bin/sh";
        let entry = parse_passwd_entry(line).unwrap();
        assert_eq!(entry.username, "root");
        assert_eq!(entry.uid, 0);
        assert_eq!(entry.gid, 0);
        assert_eq!(entry.home_dir, PathBuf::from("/root"));
        assert_eq!(entry.shell, PathBuf::from("/bin/sh"));
    }

    #[test]
    fn test_parse_passwd_entry_normal_user() {
        let line = "john:x:1000:1000:John Doe:/home/john:/bin/bash";
        let entry = parse_passwd_entry(line).unwrap();
        assert_eq!(entry.username, "john");
        assert_eq!(entry.uid, 1000);
        assert_eq!(entry.gid, 1000);
        assert_eq!(entry.gecos, "John Doe");
        assert_eq!(entry.home_dir, PathBuf::from("/home/john"));
    }

    #[test]
    fn test_parse_passwd_entry_short() {
        let line = "bad:entry";
        assert!(parse_passwd_entry(line).is_none());
    }

    #[test]
    fn test_parse_shadow_entry() {
        let line = "root:$6$salt$hash:19000:0:99999:7:::";
        let entry = parse_shadow_entry(line).unwrap();
        assert_eq!(entry.username, "root");
        assert_eq!(entry.password_hash, "$6$salt$hash");
        assert_eq!(entry.last_changed, 19000);
    }

    #[test]
    fn test_parse_shadow_entry_locked() {
        let line = "locked:!:19000:0:99999:7:::";
        let entry = parse_shadow_entry(line).unwrap();
        assert_eq!(entry.password_hash, "!");
    }

    /// Hash `password` the way `passwd` and `chpasswd` now do, so the tests
    /// below check the entry those tools actually write rather than one
    /// hand-assembled here.
    fn shadow_entry_for(password: &str) -> String {
        let mut sb = posix::crypt::buf();
        let setting =
            posix::crypt::setting_into(posix::crypt::Method::Sha512, b"0123456789abcdef", &mut sb)
                .expect("setting");
        let mut hb = posix::crypt::buf();
        posix::crypt::hash_into(password.as_bytes(), setting.as_bytes(), &mut hb)
            .expect("hash")
            .to_string()
    }

    #[test]
    fn test_check_password_locked() {
        assert_eq!(check_password("anything", "!"), PasswordCheck::Locked);
        assert_eq!(check_password("anything", "!!"), PasswordCheck::Locked);
        assert_eq!(check_password("anything", "*"), PasswordCheck::Locked);
    }

    /// A `!`-prefixed hash is the shadow-suite's way of locking an account
    /// without discarding its password.  It must not authenticate — and in
    /// particular the `!` must not be mistaken for part of the setting.
    #[test]
    fn test_check_password_locked_hash_does_not_authenticate() {
        let locked = format!("!{}", shadow_entry_for("correct horse"));
        assert_eq!(
            check_password("correct horse", &locked),
            PasswordCheck::Locked
        );
    }

    #[test]
    fn test_check_password_empty_hash() {
        assert_eq!(check_password("", ""), PasswordCheck::Accepted);
        assert_eq!(check_password("anything", ""), PasswordCheck::Rejected);
    }

    /// There is no cleartext fallback: an entry that is not a hash cannot
    /// authenticate anything, least of all itself.
    #[test]
    fn test_check_password_has_no_cleartext_path() {
        assert_eq!(check_password("secret", "secret"), PasswordCheck::Unusable);
        assert_eq!(check_password("wrong", "secret"), PasswordCheck::Unusable);
        assert_eq!(check_password("x", "x"), PasswordCheck::Unusable);
    }

    /// The regression for `requests/c-b-passwd-and-login-disagree-about-etc-shadow.md`:
    /// the entry `passwd` writes must be the entry `login` accepts.  Before
    /// the fix each tool implemented the format itself and the correct
    /// password was rejected.
    #[test]
    fn test_a_password_set_by_passwd_is_accepted_by_login() {
        let stored = shadow_entry_for("correct horse");
        assert!(stored.starts_with("$6$"));
        assert_eq!(
            check_password("correct horse", &stored),
            PasswordCheck::Accepted
        );
        assert_eq!(
            check_password("correct hors", &stored),
            PasswordCheck::Rejected
        );
        assert_eq!(check_password("", &stored), PasswordCheck::Rejected);
    }

    /// The two formats this tree wrote before it called `crypt`: `passwd`'s
    /// invented `$sha256$`, and `chpasswd`'s 64 hex digits mislabelled `$5$`.
    /// Neither can ever verify, and both must be reported as broken rather
    /// than counted as a wrong password — otherwise the administrator's only
    /// symptom is a user who insists their password is right.
    #[test]
    fn test_check_password_reports_the_obsolete_formats_as_unusable() {
        let digest = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        for prefix in ["$sha256$", "$5$", "$6$", "$1$"] {
            let stored = format!("{prefix}0123456789abcdef${digest}");
            assert_eq!(
                check_password("correct horse", &stored),
                PasswordCheck::Unusable,
                "{stored}"
            );
        }
    }

    /// A published vector, checked through the exact path `login` uses.
    ///
    /// This replaces two tests that asserted only that the old `simple_hash`
    /// was deterministic and that two inputs gave two outputs.  Both are
    /// true of every function anyone would write by accident, which is
    /// precisely why they passed while the thing under test was not a hash
    /// at all.  A known answer is the only test that can tell an algorithm
    /// from something that merely looks like one, so this file now carries
    /// one — from Ulrich Drepper's SHA-crypt specification.
    #[test]
    fn test_login_verifies_against_a_published_vector() {
        const VECTOR: &str = "$6$saltstring$svn8UoSVapNtMuq1ukKS4tPQd8iKwSMHWjl/O817G3uBnIFNjnQJuesI68u4OTLiBFdcbYEdFCoEOfaS35inz1";
        assert_eq!(
            check_password("Hello world!", VECTOR),
            PasswordCheck::Accepted
        );
        assert_eq!(
            check_password("Hello world", VECTOR),
            PasswordCheck::Rejected
        );
    }

    #[test]
    fn test_build_environment_root() {
        let user = PasswdEntry {
            username: "root".to_string(),
            uid: 0,
            gid: 0,
            gecos: String::new(),
            home_dir: PathBuf::from("/root"),
            shell: PathBuf::from("/bin/sh"),
        };
        let env = build_environment(&user, false);
        assert_eq!(env.get("HOME").unwrap(), "/root");
        assert_eq!(env.get("SHELL").unwrap(), "/bin/sh");
        assert_eq!(env.get("USER").unwrap(), "root");
        assert_eq!(env.get("LOGNAME").unwrap(), "root");
        assert!(env.get("PATH").unwrap().contains("sbin"));
        assert_eq!(env.get("MAIL").unwrap(), "/var/mail/root");
    }

    #[test]
    fn test_build_environment_normal() {
        let user = PasswdEntry {
            username: "john".to_string(),
            uid: 1000,
            gid: 1000,
            gecos: String::new(),
            home_dir: PathBuf::from("/home/john"),
            shell: PathBuf::from("/bin/bash"),
        };
        let env = build_environment(&user, false);
        assert_eq!(env.get("HOME").unwrap(), "/home/john");
        assert!(!env.get("PATH").unwrap().contains("sbin"));
    }

    #[test]
    fn test_build_environment_preserve() {
        let user = PasswdEntry {
            username: "john".to_string(),
            uid: 1000,
            gid: 1000,
            gecos: String::new(),
            home_dir: PathBuf::from("/home/john"),
            shell: PathBuf::from("/bin/bash"),
        };
        let env = build_environment(&user, true);
        assert_eq!(env.get("HOME").unwrap(), "/home/john");
        assert_eq!(env.get("USER").unwrap(), "john");
    }

    #[test]
    fn test_check_account_expired_locked() {
        let shadow = ShadowEntry {
            username: "locked".to_string(),
            password_hash: "!locked".to_string(),
            last_changed: 0,
            min_days: 0,
            max_days: 99999,
            warn_days: 7,
            inactive_days: -1,
            expire_date: -1,
        };
        let result = check_account_expired(&shadow);
        assert!(result.is_err());
    }

    #[test]
    fn test_check_account_expired_ok() {
        let shadow = ShadowEntry {
            username: "user".to_string(),
            password_hash: "$6$salt$hash".to_string(),
            last_changed: 0,
            min_days: 0,
            max_days: 99999,
            warn_days: 7,
            inactive_days: -1,
            expire_date: -1,
        };
        let result = check_account_expired(&shadow);
        assert!(result.is_ok());
    }

    #[test]
    fn test_check_account_expired_date() {
        let shadow = ShadowEntry {
            username: "user".to_string(),
            password_hash: "$6$salt$hash".to_string(),
            last_changed: 0,
            min_days: 0,
            max_days: 99999,
            warn_days: 7,
            inactive_days: -1,
            expire_date: 1, // expired
        };
        let result = check_account_expired(&shadow);
        assert!(result.is_err());
    }

    #[test]
    fn test_check_nologin_nonexistent() {
        // /etc/nologin doesn't exist on the test system
        assert!(check_nologin(1000).is_ok());
    }

    #[test]
    fn test_check_nologin_root_always_ok() {
        assert!(check_nologin(0).is_ok());
    }

    #[test]
    fn test_check_securetty_non_root() {
        assert!(check_securetty(1000, "tty1").is_ok());
    }

    #[test]
    fn test_login_error_display() {
        assert!(format!("{}", LoginError::AuthFailed("test".into())).contains("test"));
        assert!(format!("{}", LoginError::AccountLocked("locked".into())).contains("locked"));
        assert!(format!("{}", LoginError::Timeout).contains("timed out"));
    }

    #[test]
    fn test_do_login_force_with_unknown_user() {
        let cfg = Config {
            username: Some("nonexistent_user_xyz".to_string()),
            force_login: true,
            ..Default::default()
        };
        let input = b"";
        let mut reader = Cursor::new(input.as_slice());
        let mut writer = Vec::new();
        let result = do_login(&cfg, &mut reader, &mut writer);
        // Should fail because user doesn't exist in /etc/passwd
        assert!(result.is_err());
    }

    #[test]
    fn test_default_config() {
        let cfg = Config::default();
        assert_eq!(cfg.username, None);
        assert!(!cfg.force_login);
        assert!(!cfg.preserve_env);
        assert_eq!(cfg.hostname, None);
    }

    #[test]
    fn test_parse_args_combined() {
        let args = vec![
            "login".to_string(),
            "-f".to_string(),
            "-p".to_string(),
            "-h".to_string(),
            "host1".to_string(),
            "admin".to_string(),
        ];
        let cfg = parse_args(&args).unwrap();
        assert!(cfg.force_login);
        assert!(cfg.preserve_env);
        assert_eq!(cfg.hostname, Some("host1".to_string()));
        assert_eq!(cfg.username, Some("admin".to_string()));
    }

    /// Every method a `shadow(5)` entry can name must round-trip, so that
    /// upgrading the default (or reading a file another system wrote) does
    /// not quietly stop authenticating.
    #[test]
    fn test_check_password_accepts_every_supported_method() {
        for method in [
            posix::crypt::Method::Md5,
            posix::crypt::Method::Sha256,
            posix::crypt::Method::Sha512,
        ] {
            let mut sb = posix::crypt::buf();
            let setting = posix::crypt::setting_into(method, b"mysalt", &mut sb)
                .unwrap_or_else(|| panic!("{method:?} setting"));
            let mut hb = posix::crypt::buf();
            let stored = posix::crypt::hash_into(b"test", setting.as_bytes(), &mut hb)
                .unwrap_or_else(|| panic!("{method:?} hash"));
            assert!(stored.starts_with(method.prefix()), "{stored}");
            assert_eq!(check_password("test", stored), PasswordCheck::Accepted);
            assert_eq!(check_password("wrong", stored), PasswordCheck::Rejected);
        }
    }

    /// An explicit `rounds=` field is part of the format and must survive
    /// verification: the setting is read back out of the stored entry, so a
    /// parser that lost the field would recompute a different hash.
    #[test]
    fn test_check_password_honours_an_explicit_rounds_field() {
        let mut hb = posix::crypt::buf();
        let stored =
            posix::crypt::hash_into(b"test", b"$6$rounds=1234$mysalt$", &mut hb).expect("hash");
        assert!(stored.starts_with("$6$rounds=1234$mysalt$"), "{stored}");
        // The same password and salt at the default round count is a
        // *different* entry, which is the whole purpose of the field.
        let mut db = posix::crypt::buf();
        let default_rounds =
            posix::crypt::hash_into(b"test", b"$6$mysalt$", &mut db).expect("hash");
        assert_ne!(stored, default_rounds);
        assert_eq!(check_password("test", stored), PasswordCheck::Accepted);
        assert_eq!(check_password("wrong", stored), PasswordCheck::Rejected);
    }
}
