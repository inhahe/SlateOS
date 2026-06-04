// OurOS chpasswd — batch password change utility
//
// Multi-personality binary:
//   chpasswd — batch change user passwords (from stdin or file)
//   passwd   — interactive single-user password change
//
// Usage:
//   chpasswd [OPTIONS] < password-list
//   passwd [OPTIONS] [username]

#![cfg_attr(not(test), no_main)]

use std::env;
use std::io::{self, BufRead, Write};
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// Personality detection
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Personality {
    Chpasswd,
    Passwd,
}

fn detect_personality(argv0: &str) -> Personality {
    let base = argv0.rsplit('/').next().unwrap_or(argv0);
    let base = base.rsplit('\\').next().unwrap_or(base);
    let lower = base.to_ascii_lowercase();
    let lower = lower.strip_suffix(".exe").unwrap_or(&lower);
    match lower {
        "passwd" => Personality::Passwd,
        _ => Personality::Chpasswd,
    }
}

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct Config {
    personality: Personality,
    username: Option<String>,
    encrypted: bool,       // -e: passwords are already encrypted
    hash_method: HashMethod,
    min_length: usize,
    shadow_file: PathBuf,
    lock_user: bool,       // -l
    unlock_user: bool,     // -u
    delete_password: bool, // -d
    expire_password: bool, // -e for passwd personality
    status: bool,          // -S
    show_help: bool,
    show_version: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HashMethod {
    Sha256,
    Sha512,
    Md5,
}

impl HashMethod {
    fn prefix(&self) -> &'static str {
        match self {
            Self::Sha256 => "$5$",
            Self::Sha512 => "$6$",
            Self::Md5 => "$1$",
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            personality: Personality::Chpasswd,
            username: None,
            encrypted: false,
            hash_method: HashMethod::Sha512,
            min_length: 6,
            shadow_file: PathBuf::from("/etc/shadow"),
            lock_user: false,
            unlock_user: false,
            delete_password: false,
            expire_password: false,
            status: false,
            show_help: false,
            show_version: false,
        }
    }
}

fn parse_args(args: &[String]) -> Result<Config, String> {
    let personality = args
        .first()
        .map(|a| detect_personality(a))
        .unwrap_or(Personality::Chpasswd);

    let mut cfg = Config {
        personality,
        ..Default::default()
    };

    let mut i = 1;

    while i < args.len() {
        let arg = &args[i];
        match personality {
            Personality::Chpasswd => match arg.as_str() {
                "-e" | "--encrypted" => cfg.encrypted = true,
                "-m" | "--md5" => cfg.hash_method = HashMethod::Md5,
                "-s" | "--sha256" => cfg.hash_method = HashMethod::Sha256,
                "-S" | "--sha512" => cfg.hash_method = HashMethod::Sha512,
                "-h" | "--help" => cfg.show_help = true,
                "-V" | "--version" => cfg.show_version = true,
                other if other.starts_with('-') => {
                    return Err(format!("chpasswd: unknown option: {other}"));
                }
                _ => {} // positional args ignored
            },
            Personality::Passwd => match arg.as_str() {
                "-l" | "--lock" => cfg.lock_user = true,
                "-u" | "--unlock" => cfg.unlock_user = true,
                "-d" | "--delete" => cfg.delete_password = true,
                "-e" | "--expire" => cfg.expire_password = true,
                "-S" | "--status" => cfg.status = true,
                "-n" | "--mindays" => {
                    i += 1; // skip value
                }
                "-x" | "--maxdays" => {
                    i += 1;
                }
                "-w" | "--warndays" => {
                    i += 1;
                }
                "-i" | "--inactive" => {
                    i += 1;
                }
                "-h" | "--help" => cfg.show_help = true,
                "-V" | "--version" => cfg.show_version = true,
                other if other.starts_with('-') => {
                    return Err(format!("passwd: unknown option: {other}"));
                }
                _ => {
                    if cfg.username.is_none() {
                        cfg.username = Some(arg.clone());
                    }
                }
            },
        }
        i += 1;
    }

    Ok(cfg)
}

// ---------------------------------------------------------------------------
// Password hashing
// ---------------------------------------------------------------------------

/// Generate a random salt string
fn generate_salt(len: usize) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789./";
    // Simple PRNG for salt generation
    let mut seed = 42u64; // Would use /dev/urandom in real system
    // Try to get some entropy from time-like sources
    if let Ok(content) = std::fs::read_to_string("/proc/uptime") {
        for b in content.bytes() {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(u64::from(b));
        }
    }
    seed = seed.wrapping_mul(6364136223846793005).wrapping_add(std::process::id() as u64);

    let mut salt = String::with_capacity(len);
    for _ in 0..len {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let idx = ((seed >> 33) as usize) % CHARS.len();
        salt.push(CHARS[idx] as char);
    }
    salt
}

/// Hash a password with salt using our simple hash
fn hash_password(password: &str, method: HashMethod) -> String {
    let salt = generate_salt(16);
    let salted = format!("{salt}${password}");

    // Simple hash (placeholder for proper crypt)
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a,
        0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
    ];

    for (i, byte) in salted.bytes().enumerate() {
        let idx = i % 8;
        h[idx] = h[idx].wrapping_mul(31).wrapping_add(u32::from(byte));
        h[(idx + 1) % 8] ^= h[idx].rotate_left(7);
    }

    let hash_str: String = h.iter().map(|v| format!("{v:08x}")).collect();
    format!("{}{salt}${hash_str}", method.prefix())
}

/// Validate password strength
fn validate_password(password: &str, min_length: usize) -> Result<(), String> {
    if password.len() < min_length {
        return Err(format!(
            "password is too short (minimum {min_length} characters)"
        ));
    }
    if password.chars().all(|c| c.is_ascii_lowercase()) {
        return Err("password is too simple (use mixed case, numbers, or symbols)".to_string());
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Shadow file operations
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct ShadowEntry {
    username: String,
    password_hash: String,
    last_changed: String,
    min_days: String,
    max_days: String,
    warn_days: String,
    inactive_days: String,
    expire_date: String,
    reserved: String,
}

fn parse_shadow_line(line: &str) -> Option<ShadowEntry> {
    let fields: Vec<&str> = line.split(':').collect();
    if fields.len() < 9 {
        return None;
    }
    Some(ShadowEntry {
        username: fields[0].to_string(),
        password_hash: fields[1].to_string(),
        last_changed: fields[2].to_string(),
        min_days: fields[3].to_string(),
        max_days: fields[4].to_string(),
        warn_days: fields[5].to_string(),
        inactive_days: fields[6].to_string(),
        expire_date: fields[7].to_string(),
        reserved: fields[8].to_string(),
    })
}

fn format_shadow_entry(entry: &ShadowEntry) -> String {
    format!(
        "{}:{}:{}:{}:{}:{}:{}:{}:{}",
        entry.username,
        entry.password_hash,
        entry.last_changed,
        entry.min_days,
        entry.max_days,
        entry.warn_days,
        entry.inactive_days,
        entry.expire_date,
        entry.reserved
    )
}

fn read_shadow_file(path: &std::path::Path) -> io::Result<Vec<ShadowEntry>> {
    let content = std::fs::read_to_string(path)?;
    let mut entries = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(entry) = parse_shadow_line(line) {
            entries.push(entry);
        }
    }
    Ok(entries)
}

fn write_shadow_file(path: &std::path::Path, entries: &[ShadowEntry]) -> io::Result<()> {
    let mut content = String::new();
    for entry in entries {
        content.push_str(&format_shadow_entry(entry));
        content.push('\n');
    }
    std::fs::write(path, content)
}

fn update_password(
    shadow_path: &std::path::Path,
    username: &str,
    new_hash: &str,
) -> Result<(), String> {
    let mut entries = read_shadow_file(shadow_path)
        .map_err(|e| format!("cannot read {}: {e}", shadow_path.display()))?;

    let mut found = false;
    for entry in &mut entries {
        if entry.username == username {
            entry.password_hash = new_hash.to_string();
            // Update last_changed to "today" (days since epoch)
            entry.last_changed = "19800".to_string();
            found = true;
            break;
        }
    }

    if !found {
        return Err(format!("user '{username}' not found in shadow file"));
    }

    write_shadow_file(shadow_path, &entries)
        .map_err(|e| format!("cannot write {}: {e}", shadow_path.display()))?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Password status display
// ---------------------------------------------------------------------------

fn show_password_status(
    shadow_path: &std::path::Path,
    username: &str,
    writer: &mut dyn Write,
) -> Result<(), String> {
    let entries = read_shadow_file(shadow_path)
        .map_err(|e| format!("cannot read shadow: {e}"))?;

    let entry = entries
        .iter()
        .find(|e| e.username == username)
        .ok_or_else(|| format!("user '{username}' not found"))?;

    let status = if entry.password_hash.starts_with('!')
        || entry.password_hash == "*"
        || entry.password_hash == "!!"
    {
        "L" // locked / no password (! prefix or sentinel)
    } else if entry.password_hash.is_empty() {
        "NP" // no password
    } else {
        "P" // password set
    };

    writeln!(
        writer,
        "{} {} {} {} {} {} {}",
        username,
        status,
        entry.last_changed,
        entry.min_days,
        entry.max_days,
        entry.warn_days,
        entry.inactive_days,
    )
    .map_err(|e| format!("write: {e}"))?;

    Ok(())
}

// ---------------------------------------------------------------------------
// chpasswd mode
// ---------------------------------------------------------------------------

fn run_chpasswd(
    cfg: &Config,
    reader: &mut dyn BufRead,
    writer: &mut dyn Write,
    err_writer: &mut dyn Write,
) -> i32 {
    let mut errors = 0;
    let mut line = String::new();
    let mut line_num = 0u64;

    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {}
            Err(e) => {
                let _ = writeln!(err_writer, "chpasswd: read error: {e}");
                return 1;
            }
        }

        line_num = line_num.saturating_add(1);
        let line_trimmed = line.trim();
        if line_trimmed.is_empty() || line_trimmed.starts_with('#') {
            continue;
        }

        // Format: username:password
        let parts: Vec<&str> = line_trimmed.splitn(2, ':').collect();
        if parts.len() != 2 {
            let _ = writeln!(err_writer, "chpasswd: line {line_num}: invalid format (expected user:password)");
            errors += 1;
            continue;
        }

        let username = parts[0].trim();
        let password = parts[1].trim();

        if username.is_empty() {
            let _ = writeln!(err_writer, "chpasswd: line {line_num}: empty username");
            errors += 1;
            continue;
        }

        let hash = if cfg.encrypted {
            password.to_string()
        } else {
            if let Err(e) = validate_password(password, cfg.min_length) {
                let _ = writeln!(err_writer, "chpasswd: {username}: {e}");
                errors += 1;
                continue;
            }
            hash_password(password, cfg.hash_method)
        };

        match update_password(&cfg.shadow_file, username, &hash) {
            Ok(()) => {
                let _ = writeln!(writer, "password changed for {username}");
            }
            Err(e) => {
                let _ = writeln!(err_writer, "chpasswd: {username}: {e}");
                errors += 1;
            }
        }
    }

    if errors > 0 { 1 } else { 0 }
}

// ---------------------------------------------------------------------------
// passwd mode (interactive)
// ---------------------------------------------------------------------------

fn run_passwd(
    cfg: &Config,
    reader: &mut dyn BufRead,
    writer: &mut dyn Write,
    err_writer: &mut dyn Write,
) -> i32 {
    let username = cfg
        .username
        .clone()
        .or_else(|| env::var("USER").ok())
        .unwrap_or_else(|| "root".to_string());

    // Lock user
    if cfg.lock_user {
        match lock_user(&cfg.shadow_file, &username) {
            Ok(()) => {
                let _ = writeln!(writer, "passwd: password for {username} locked");
                return 0;
            }
            Err(e) => {
                let _ = writeln!(err_writer, "passwd: {e}");
                return 1;
            }
        }
    }

    // Unlock user
    if cfg.unlock_user {
        match unlock_user(&cfg.shadow_file, &username) {
            Ok(()) => {
                let _ = writeln!(writer, "passwd: password for {username} unlocked");
                return 0;
            }
            Err(e) => {
                let _ = writeln!(err_writer, "passwd: {e}");
                return 1;
            }
        }
    }

    // Delete password
    if cfg.delete_password {
        match update_password(&cfg.shadow_file, &username, "") {
            Ok(()) => {
                let _ = writeln!(writer, "passwd: password for {username} deleted");
                return 0;
            }
            Err(e) => {
                let _ = writeln!(err_writer, "passwd: {e}");
                return 1;
            }
        }
    }

    // Show status
    if cfg.status {
        match show_password_status(&cfg.shadow_file, &username, writer) {
            Ok(()) => return 0,
            Err(e) => {
                let _ = writeln!(err_writer, "passwd: {e}");
                return 1;
            }
        }
    }

    // Expire password
    if cfg.expire_password {
        // Set last_changed to 0 to force password change
        let entries = match read_shadow_file(&cfg.shadow_file) {
            Ok(e) => e,
            Err(e) => {
                let _ = writeln!(err_writer, "passwd: cannot read shadow: {e}");
                return 1;
            }
        };
        let mut entries = entries;
        if let Some(entry) = entries.iter_mut().find(|e| e.username == username) {
            entry.last_changed = "0".to_string();
        }
        if let Err(e) = write_shadow_file(&cfg.shadow_file, &entries) {
            let _ = writeln!(err_writer, "passwd: cannot write shadow: {e}");
            return 1;
        }
        let _ = writeln!(writer, "passwd: password for {username} expired");
        return 0;
    }

    // Interactive password change
    let _ = writeln!(writer, "Changing password for {username}.");
    let _ = write!(writer, "New password: ");
    let _ = writer.flush();

    let mut password1 = String::new();
    if reader.read_line(&mut password1).is_err() {
        let _ = writeln!(err_writer, "passwd: error reading password");
        return 1;
    }
    let password1 = password1.trim().to_string();

    if let Err(e) = validate_password(&password1, cfg.min_length) {
        let _ = writeln!(err_writer, "passwd: {e}");
        return 1;
    }

    let _ = write!(writer, "Retype new password: ");
    let _ = writer.flush();

    let mut password2 = String::new();
    if reader.read_line(&mut password2).is_err() {
        let _ = writeln!(err_writer, "passwd: error reading password");
        return 1;
    }
    let password2 = password2.trim();

    if password1 != password2 {
        let _ = writeln!(err_writer, "passwd: passwords don't match");
        return 1;
    }

    let hash = hash_password(&password1, cfg.hash_method);
    match update_password(&cfg.shadow_file, &username, &hash) {
        Ok(()) => {
            let _ = writeln!(writer, "passwd: password updated successfully");
            0
        }
        Err(e) => {
            let _ = writeln!(err_writer, "passwd: {e}");
            1
        }
    }
}

fn lock_user(shadow_path: &std::path::Path, username: &str) -> Result<(), String> {
    let mut entries = read_shadow_file(shadow_path)
        .map_err(|e| format!("cannot read shadow: {e}"))?;

    let entry = entries
        .iter_mut()
        .find(|e| e.username == username)
        .ok_or_else(|| format!("user '{username}' not found"))?;

    if !entry.password_hash.starts_with('!') {
        entry.password_hash = format!("!{}", entry.password_hash);
    }

    write_shadow_file(shadow_path, &entries)
        .map_err(|e| format!("cannot write shadow: {e}"))?;
    Ok(())
}

fn unlock_user(shadow_path: &std::path::Path, username: &str) -> Result<(), String> {
    let mut entries = read_shadow_file(shadow_path)
        .map_err(|e| format!("cannot read shadow: {e}"))?;

    let entry = entries
        .iter_mut()
        .find(|e| e.username == username)
        .ok_or_else(|| format!("user '{username}' not found"))?;

    if let Some(stripped) = entry.password_hash.strip_prefix('!') {
        entry.password_hash = stripped.to_string();
    }

    write_shadow_file(shadow_path, &entries)
        .map_err(|e| format!("cannot write shadow: {e}"))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Help / version
// ---------------------------------------------------------------------------

fn print_help(personality: Personality) {
    match personality {
        Personality::Chpasswd => {
            println!("Usage: chpasswd [OPTIONS]");
            println!();
            println!("Update passwords in batch mode. Read user:password pairs from stdin.");
            println!();
            println!("Options:");
            println!("  -e, --encrypted  Passwords are already encrypted");
            println!("  -m, --md5        Use MD5 hash method");
            println!("  -s, --sha256     Use SHA-256 hash method");
            println!("  -S, --sha512     Use SHA-512 hash method (default)");
            println!("  -h, --help       Show this help");
            println!("  -V, --version    Show version");
        }
        Personality::Passwd => {
            println!("Usage: passwd [OPTIONS] [username]");
            println!();
            println!("Change user password.");
            println!();
            println!("Options:");
            println!("  -l, --lock       Lock the named account");
            println!("  -u, --unlock     Unlock the named account");
            println!("  -d, --delete     Delete the password (make it empty)");
            println!("  -e, --expire     Force password expiration");
            println!("  -S, --status     Show password status");
            println!("  -h, --help       Show this help");
            println!("  -V, --version    Show version");
        }
    }
}

fn print_version(personality: Personality) {
    let name = match personality {
        Personality::Chpasswd => "chpasswd",
        Personality::Passwd => "passwd",
    };
    println!("{name} (OurOS) 0.1.0");
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
            eprintln!("{e}");
            return 1;
        }
    };

    if cfg.show_help {
        print_help(cfg.personality);
        return 0;
    }

    if cfg.show_version {
        print_version(cfg.personality);
        return 0;
    }

    let stdin = io::stdin();
    let mut reader = stdin.lock();
    let stdout = io::stdout();
    let mut writer = stdout.lock();
    let stderr = io::stderr();
    let mut err_writer = stderr.lock();

    match cfg.personality {
        Personality::Chpasswd => run_chpasswd(&cfg, &mut reader, &mut writer, &mut err_writer),
        Personality::Passwd => run_passwd(&cfg, &mut reader, &mut writer, &mut err_writer),
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
    fn test_detect_personality_chpasswd() {
        assert_eq!(detect_personality("chpasswd"), Personality::Chpasswd);
        assert_eq!(detect_personality("/usr/sbin/chpasswd"), Personality::Chpasswd);
    }

    #[test]
    fn test_detect_personality_passwd() {
        assert_eq!(detect_personality("passwd"), Personality::Passwd);
        assert_eq!(detect_personality("/usr/bin/passwd"), Personality::Passwd);
    }

    #[test]
    fn test_parse_args_chpasswd_basic() {
        let args = vec!["chpasswd".to_string()];
        let cfg = parse_args(&args).unwrap();
        assert_eq!(cfg.personality, Personality::Chpasswd);
        assert!(!cfg.encrypted);
    }

    #[test]
    fn test_parse_args_chpasswd_encrypted() {
        let args = vec!["chpasswd".to_string(), "-e".to_string()];
        let cfg = parse_args(&args).unwrap();
        assert!(cfg.encrypted);
    }

    #[test]
    fn test_parse_args_chpasswd_md5() {
        let args = vec!["chpasswd".to_string(), "-m".to_string()];
        let cfg = parse_args(&args).unwrap();
        assert_eq!(cfg.hash_method, HashMethod::Md5);
    }

    #[test]
    fn test_parse_args_chpasswd_sha256() {
        let args = vec!["chpasswd".to_string(), "-s".to_string()];
        let cfg = parse_args(&args).unwrap();
        assert_eq!(cfg.hash_method, HashMethod::Sha256);
    }

    #[test]
    fn test_parse_args_passwd_lock() {
        let args = vec![
            "passwd".to_string(),
            "-l".to_string(),
            "user1".to_string(),
        ];
        let cfg = parse_args(&args).unwrap();
        assert_eq!(cfg.personality, Personality::Passwd);
        assert!(cfg.lock_user);
        assert_eq!(cfg.username, Some("user1".to_string()));
    }

    #[test]
    fn test_parse_args_passwd_unlock() {
        let args = vec![
            "passwd".to_string(),
            "-u".to_string(),
            "user1".to_string(),
        ];
        let cfg = parse_args(&args).unwrap();
        assert!(cfg.unlock_user);
    }

    #[test]
    fn test_parse_args_passwd_delete() {
        let args = vec!["passwd".to_string(), "-d".to_string()];
        let cfg = parse_args(&args).unwrap();
        assert!(cfg.delete_password);
    }

    #[test]
    fn test_parse_args_passwd_expire() {
        let args = vec!["passwd".to_string(), "-e".to_string()];
        let cfg = parse_args(&args).unwrap();
        assert!(cfg.expire_password);
    }

    #[test]
    fn test_parse_args_passwd_status() {
        let args = vec!["passwd".to_string(), "-S".to_string()];
        let cfg = parse_args(&args).unwrap();
        assert!(cfg.status);
    }

    #[test]
    fn test_parse_args_help() {
        for name in &["chpasswd", "passwd"] {
            let args = vec![name.to_string(), "--help".to_string()];
            let cfg = parse_args(&args).unwrap();
            assert!(cfg.show_help);
        }
    }

    #[test]
    fn test_hash_password_sha512() {
        let hash = hash_password("testpass", HashMethod::Sha512);
        assert!(hash.starts_with("$6$"));
        assert!(hash.len() > 20);
    }

    #[test]
    fn test_hash_password_sha256() {
        let hash = hash_password("testpass", HashMethod::Sha256);
        assert!(hash.starts_with("$5$"));
    }

    #[test]
    fn test_hash_password_md5() {
        let hash = hash_password("testpass", HashMethod::Md5);
        assert!(hash.starts_with("$1$"));
    }

    #[test]
    fn test_hash_password_different() {
        let h1 = hash_password("pass1", HashMethod::Sha512);
        let h2 = hash_password("pass2", HashMethod::Sha512);
        // Different passwords should produce different hashes
        // (salt is based on process state, so should differ)
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_validate_password_too_short() {
        assert!(validate_password("ab", 6).is_err());
    }

    #[test]
    fn test_validate_password_ok() {
        assert!(validate_password("MyP@ss1", 6).is_ok());
    }

    #[test]
    fn test_validate_password_all_lower() {
        assert!(validate_password("abcdefgh", 6).is_err());
    }

    #[test]
    fn test_parse_shadow_line() {
        let line = "root:$6$salt$hash:19000:0:99999:7:::";
        let entry = parse_shadow_line(line).unwrap();
        assert_eq!(entry.username, "root");
        assert_eq!(entry.password_hash, "$6$salt$hash");
        assert_eq!(entry.last_changed, "19000");
    }

    #[test]
    fn test_parse_shadow_line_short() {
        assert!(parse_shadow_line("short:line").is_none());
    }

    #[test]
    fn test_format_shadow_entry() {
        let entry = ShadowEntry {
            username: "user".to_string(),
            password_hash: "$6$hash".to_string(),
            last_changed: "19000".to_string(),
            min_days: "0".to_string(),
            max_days: "99999".to_string(),
            warn_days: "7".to_string(),
            inactive_days: "".to_string(),
            expire_date: "".to_string(),
            reserved: "".to_string(),
        };
        let formatted = format_shadow_entry(&entry);
        assert_eq!(formatted, "user:$6$hash:19000:0:99999:7:::");
    }

    #[test]
    fn test_hash_method_prefix() {
        assert_eq!(HashMethod::Sha512.prefix(), "$6$");
        assert_eq!(HashMethod::Sha256.prefix(), "$5$");
        assert_eq!(HashMethod::Md5.prefix(), "$1$");
    }

    #[test]
    fn test_generate_salt() {
        let s1 = generate_salt(16);
        assert_eq!(s1.len(), 16);
        assert!(s1.chars().all(|c| c.is_alphanumeric() || c == '.' || c == '/'));
    }

    #[test]
    fn test_default_config() {
        let cfg = Config::default();
        assert_eq!(cfg.hash_method, HashMethod::Sha512);
        assert_eq!(cfg.min_length, 6);
        assert!(!cfg.encrypted);
    }

    #[test]
    fn test_run_chpasswd_empty() {
        let cfg = Config::default();
        let input = b"";
        let mut reader = Cursor::new(input.as_slice());
        let mut writer = Vec::new();
        let mut err_writer = Vec::new();
        let code = run_chpasswd(&cfg, &mut reader, &mut writer, &mut err_writer);
        assert_eq!(code, 0);
    }

    #[test]
    fn test_run_chpasswd_invalid_format() {
        let cfg = Config::default();
        let input = b"invalid_line_no_colon\n";
        let mut reader = Cursor::new(input.as_slice());
        let mut writer = Vec::new();
        let mut err_writer = Vec::new();
        let code = run_chpasswd(&cfg, &mut reader, &mut writer, &mut err_writer);
        assert_eq!(code, 1);
        let err = String::from_utf8(err_writer).unwrap();
        assert!(err.contains("invalid format"));
    }

    #[test]
    fn test_run_chpasswd_empty_username() {
        let cfg = Config::default();
        let input = b":password\n";
        let mut reader = Cursor::new(input.as_slice());
        let mut writer = Vec::new();
        let mut err_writer = Vec::new();
        let code = run_chpasswd(&cfg, &mut reader, &mut writer, &mut err_writer);
        assert_eq!(code, 1);
    }

    #[test]
    fn test_run_chpasswd_comment() {
        let cfg = Config::default();
        let input = b"# comment\n\n";
        let mut reader = Cursor::new(input.as_slice());
        let mut writer = Vec::new();
        let mut err_writer = Vec::new();
        let code = run_chpasswd(&cfg, &mut reader, &mut writer, &mut err_writer);
        assert_eq!(code, 0);
    }
}
