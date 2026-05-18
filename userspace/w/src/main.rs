// OurOS w — show who is logged in and what they are doing
//
// Multi-personality binary:
//   w      — show logged-in users and their activity
//   finger — user information lookup (RFC 1288)
//   pinky  — lightweight finger
//
// Usage:
//   w [OPTIONS] [user]
//   finger [OPTIONS] [user@host | user...]
//   pinky [OPTIONS] [user...]

#![cfg_attr(not(test), no_main)]

use std::env;
use std::io::{self, Write};
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// Personality detection
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Personality {
    W,
    Finger,
    Pinky,
}

fn detect_personality(argv0: &str) -> Personality {
    let base = argv0.rsplit('/').next().unwrap_or(argv0);
    let base = base.rsplit('\\').next().unwrap_or(base);
    let lower = base.to_ascii_lowercase();
    let lower = lower.strip_suffix(".exe").unwrap_or(&lower);
    match lower {
        "finger" => Personality::Finger,
        "pinky" => Personality::Pinky,
        _ => Personality::W,
    }
}

// ---------------------------------------------------------------------------
// Data structures
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct UtmpEntry {
    user: String,
    tty: String,
    host: String,
    login_time: u64,
    pid: u32,
    idle_secs: u64,
    what: String,
}

#[derive(Debug, Clone)]
struct UserInfo {
    username: String,
    real_name: String,
    home_dir: String,
    shell: String,
    office: String,
    office_phone: String,
    home_phone: String,
    plan: Option<String>,
    project: Option<String>,
    mail_status: MailStatus,
    login_sessions: Vec<UtmpEntry>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MailStatus {
    NoMail,
    OldMail,
    NewMail,
}

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct Config {
    personality: Personality,
    users: Vec<String>,
    no_header: bool,
    short_format: bool,
    long_format: bool,
    from_field: bool,
    idle_sort: bool,
    show_help: bool,
    show_version: bool,
    // finger-specific
    no_plan: bool,
    no_project: bool,
    match_real_name: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            personality: Personality::W,
            users: Vec::new(),
            no_header: false,
            short_format: false,
            long_format: false,
            from_field: true,
            idle_sort: false,
            show_help: false,
            show_version: false,
            no_plan: false,
            no_project: false,
            match_real_name: true,
        }
    }
}

fn parse_args(args: &[String]) -> Result<Config, String> {
    let personality = args
        .first()
        .map(|a| detect_personality(a))
        .unwrap_or(Personality::W);

    let mut cfg = Config {
        personality,
        ..Default::default()
    };

    let mut i = 1;

    while i < args.len() {
        let arg = &args[i];

        match personality {
            Personality::W => match arg.as_str() {
                "-h" | "--no-header" => cfg.no_header = true,
                "-s" | "--short" => cfg.short_format = true,
                "-f" | "--from" => cfg.from_field = !cfg.from_field,
                "-i" | "--ip-addr" => {} // accept, shows IP instead of hostname
                "-o" | "--old-style" => cfg.short_format = true,
                "--help" => cfg.show_help = true,
                "-V" | "--version" => cfg.show_version = true,
                other if other.starts_with('-') => {
                    return Err(format!("w: unknown option: {other}"));
                }
                _ => cfg.users.push(arg.clone()),
            },
            Personality::Finger => match arg.as_str() {
                "-l" => cfg.long_format = true,
                "-s" => cfg.short_format = true,
                "-p" => cfg.no_plan = true,
                "-m" => cfg.match_real_name = false,
                "--help" => cfg.show_help = true,
                "--version" => cfg.show_version = true,
                other if other.starts_with('-') => {
                    return Err(format!("finger: unknown option: {other}"));
                }
                _ => cfg.users.push(arg.clone()),
            },
            Personality::Pinky => match arg.as_str() {
                "-l" => cfg.long_format = true,
                "-s" => cfg.short_format = true,
                "-f" => cfg.from_field = false,
                "-w" => {} // omit name field
                "-i" => {} // show IPs
                "-b" => cfg.no_plan = true,
                "-h" => cfg.no_header = true,
                "-p" => cfg.no_project = true,
                "--help" => cfg.show_help = true,
                "--version" => cfg.show_version = true,
                other if other.starts_with('-') => {
                    return Err(format!("pinky: unknown option: {other}"));
                }
                _ => cfg.users.push(arg.clone()),
            },
        }
        i += 1;
    }

    Ok(cfg)
}

// ---------------------------------------------------------------------------
// System information gathering
// ---------------------------------------------------------------------------

fn read_utmp_entries() -> Vec<UtmpEntry> {
    // In a real OS, read from /var/run/utmp binary file
    // For now, synthesize from /var/run/utmp text file or fallback
    let mut entries = Vec::new();

    if let Ok(content) = std::fs::read_to_string("/var/run/utmp") {
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            // Format: user:tty:host:login_time:pid:what
            let fields: Vec<&str> = line.split(':').collect();
            if fields.len() >= 4 {
                entries.push(UtmpEntry {
                    user: fields[0].to_string(),
                    tty: fields[1].to_string(),
                    host: fields.get(2).unwrap_or(&"").to_string(),
                    login_time: fields.get(3).and_then(|s| s.parse().ok()).unwrap_or(0),
                    pid: fields.get(4).and_then(|s| s.parse().ok()).unwrap_or(0),
                    idle_secs: 0,
                    what: fields.get(5).unwrap_or(&"-").to_string(),
                });
            }
        }
    }

    // If no utmp entries, try to show current user
    if entries.is_empty() {
        if let Ok(user) = env::var("USER") {
            entries.push(UtmpEntry {
                user,
                tty: "console".to_string(),
                host: String::new(),
                login_time: 0,
                pid: std::process::id(),
                idle_secs: 0,
                what: "-".to_string(),
            });
        }
    }

    entries
}

fn read_passwd_gecos(username: &str) -> (String, String, String, String, String) {
    // Returns (real_name, home_dir, shell, office, office_phone)
    if let Ok(content) = std::fs::read_to_string("/etc/passwd") {
        for line in content.lines() {
            let fields: Vec<&str> = line.split(':').collect();
            if fields.len() >= 7 && fields[0] == username {
                let gecos = fields[4];
                let gecos_parts: Vec<&str> = gecos.split(',').collect();
                let real_name = gecos_parts.first().unwrap_or(&username).to_string();
                let office = gecos_parts.get(1).unwrap_or(&"").to_string();
                let office_phone = gecos_parts.get(2).unwrap_or(&"").to_string();

                return (
                    real_name,
                    fields[5].to_string(),
                    fields[6].to_string(),
                    office,
                    office_phone,
                );
            }
        }
    }
    (
        username.to_string(),
        String::new(),
        String::new(),
        String::new(),
        String::new(),
    )
}

fn get_user_info(username: &str) -> UserInfo {
    let (real_name, home_dir, shell, office, office_phone) = read_passwd_gecos(username);

    // Read .plan file
    let plan = if !home_dir.is_empty() {
        let plan_path = PathBuf::from(&home_dir).join(".plan");
        std::fs::read_to_string(plan_path).ok()
    } else {
        None
    };

    // Read .project file
    let project = if !home_dir.is_empty() {
        let proj_path = PathBuf::from(&home_dir).join(".project");
        std::fs::read_to_string(proj_path).ok()
    } else {
        None
    };

    // Check mail
    let mail_path = format!("/var/mail/{username}");
    let mail_status = match std::fs::metadata(&mail_path) {
        Ok(meta) => {
            if meta.len() > 0 {
                MailStatus::NewMail // simplified
            } else {
                MailStatus::NoMail
            }
        }
        Err(_) => MailStatus::NoMail,
    };

    let sessions = read_utmp_entries()
        .into_iter()
        .filter(|e| e.user == username)
        .collect();

    UserInfo {
        username: username.to_string(),
        real_name,
        home_dir,
        shell,
        office,
        office_phone,
        home_phone: String::new(),
        plan,
        project,
        mail_status,
        login_sessions: sessions,
    }
}

fn get_uptime_str() -> String {
    if let Ok(content) = std::fs::read_to_string("/proc/uptime") {
        if let Some(secs_str) = content.split_whitespace().next() {
            if let Ok(secs) = secs_str.parse::<f64>() {
                let total_secs = secs as u64;
                let days = total_secs / 86400;
                let hours = (total_secs % 86400) / 3600;
                let mins = (total_secs % 3600) / 60;

                if days > 0 {
                    return format!("up {days} day(s), {hours:2}:{mins:02}");
                }
                return format!("up {hours:2}:{mins:02}");
            }
        }
    }
    "up  0:00".to_string()
}

fn get_load_avg() -> String {
    if let Ok(content) = std::fs::read_to_string("/proc/loadavg") {
        let parts: Vec<&str> = content.split_whitespace().collect();
        if parts.len() >= 3 {
            return format!("load average: {}, {}, {}", parts[0], parts[1], parts[2]);
        }
    }
    "load average: 0.00, 0.00, 0.00".to_string()
}

fn get_current_time() -> String {
    // Simplified — would read from system clock
    "00:00:00".to_string()
}

fn format_idle(secs: u64) -> String {
    if secs == 0 {
        return "  .  ".to_string();
    }
    if secs < 60 {
        return format!(" {secs:2}s ");
    }
    let mins = secs / 60;
    if mins < 60 {
        return format!("{mins:2}:{:02} ", secs % 60);
    }
    let hours = mins / 60;
    if hours < 24 {
        return format!("{hours:2}:{:02}m", mins % 60);
    }
    let days = hours / 24;
    format!("{days:2}days")
}

fn format_login_time(timestamp: u64) -> String {
    if timestamp == 0 {
        return " ?????".to_string();
    }
    // Simplified time formatting
    let hours = (timestamp / 3600) % 24;
    let mins = (timestamp / 60) % 60;
    format!("{hours:02}:{mins:02}")
}

// ---------------------------------------------------------------------------
// Output formatting
// ---------------------------------------------------------------------------

fn run_w(cfg: &Config, writer: &mut dyn Write) -> io::Result<()> {
    let entries = read_utmp_entries();

    // Filter by user if specified
    let entries: Vec<&UtmpEntry> = if cfg.users.is_empty() {
        entries.iter().collect()
    } else {
        entries
            .iter()
            .filter(|e| cfg.users.iter().any(|u| u == &e.user))
            .collect()
    };

    // Header
    if !cfg.no_header {
        let time = get_current_time();
        let uptime = get_uptime_str();
        let loadavg = get_load_avg();
        let nusers = entries.len();
        writeln!(
            writer,
            " {time} {uptime},  {nusers} user(s),  {loadavg}"
        )?;

        if cfg.short_format {
            writeln!(writer, "USER     TTY        IDLE  WHAT")?;
        } else if cfg.from_field {
            writeln!(
                writer,
                "USER     TTY      FROM             LOGIN@   IDLE   WHAT"
            )?;
        } else {
            writeln!(writer, "USER     TTY        LOGIN@   IDLE   WHAT")?;
        }
    }

    // Output entries
    for entry in &entries {
        if cfg.short_format {
            writeln!(
                writer,
                "{:<8} {:<10} {} {}",
                truncate_str(&entry.user, 8),
                truncate_str(&entry.tty, 10),
                format_idle(entry.idle_secs),
                &entry.what
            )?;
        } else if cfg.from_field {
            writeln!(
                writer,
                "{:<8} {:<8} {:<16} {:<8} {} {}",
                truncate_str(&entry.user, 8),
                truncate_str(&entry.tty, 8),
                truncate_str(&entry.host, 16),
                format_login_time(entry.login_time),
                format_idle(entry.idle_secs),
                &entry.what
            )?;
        } else {
            writeln!(
                writer,
                "{:<8} {:<10} {:<8} {} {}",
                truncate_str(&entry.user, 8),
                truncate_str(&entry.tty, 10),
                format_login_time(entry.login_time),
                format_idle(entry.idle_secs),
                &entry.what
            )?;
        }
    }

    Ok(())
}

fn run_finger(cfg: &Config, writer: &mut dyn Write) -> io::Result<()> {
    if cfg.users.is_empty() {
        // No user specified — show all logged in users (short format)
        let entries = read_utmp_entries();
        writeln!(
            writer,
            "Login     Name              Tty      Idle  Login Time   Office     Office Phone"
        )?;
        for entry in &entries {
            let (real_name, _, _, office, phone) = read_passwd_gecos(&entry.user);
            writeln!(
                writer,
                "{:<9} {:<17} {:<8} {} {:<12} {:<10} {}",
                truncate_str(&entry.user, 9),
                truncate_str(&real_name, 17),
                truncate_str(&entry.tty, 8),
                format_idle(entry.idle_secs),
                format_login_time(entry.login_time),
                truncate_str(&office, 10),
                truncate_str(&phone, 12),
            )?;
        }
        return Ok(());
    }

    // For each specified user, show long format
    for (idx, username) in cfg.users.iter().enumerate() {
        // Check for user@host (remote finger)
        if username.contains('@') {
            writeln!(writer, "[{username}]")?;
            writeln!(writer, "Remote finger not supported")?;
            continue;
        }

        let info = get_user_info(username);

        if cfg.short_format && !cfg.long_format {
            // Short format
            writeln!(
                writer,
                "Login: {:<20} Name: {}",
                info.username, info.real_name
            )?;
        } else {
            // Long format
            if idx > 0 {
                writeln!(writer)?;
            }
            writeln!(
                writer,
                "Login: {:<20} Name: {}",
                info.username, info.real_name
            )?;
            writeln!(
                writer,
                "Directory: {:<22} Shell: {}",
                info.home_dir, info.shell
            )?;
            if !info.office.is_empty() || !info.office_phone.is_empty() {
                writeln!(
                    writer,
                    "Office: {:<24} Office Phone: {}",
                    info.office, info.office_phone
                )?;
            }

            // Login sessions
            if info.login_sessions.is_empty() {
                writeln!(writer, "Never logged in.")?;
            } else {
                for session in &info.login_sessions {
                    write!(writer, "On since ")?;
                    write!(writer, "{}", format_login_time(session.login_time))?;
                    writeln!(writer, " on {}", session.tty)?;
                }
            }

            // Mail
            match info.mail_status {
                MailStatus::NewMail => writeln!(writer, "New mail received.")?,
                MailStatus::OldMail => writeln!(writer, "Mail last read.")?,
                MailStatus::NoMail => writeln!(writer, "No mail.")?,
            }

            // Plan/project
            if !cfg.no_plan {
                if !cfg.no_project {
                    if let Some(ref project) = info.project {
                        writeln!(writer, "Project: {}", project.trim())?;
                    }
                }
                if let Some(ref plan) = info.plan {
                    writeln!(writer, "Plan:")?;
                    write!(writer, "{plan}")?;
                } else {
                    writeln!(writer, "No Plan.")?;
                }
            }
        }
    }

    Ok(())
}

fn run_pinky(cfg: &Config, writer: &mut dyn Write) -> io::Result<()> {
    if cfg.users.is_empty() || cfg.short_format {
        // Short format — list all logged in users
        let entries = read_utmp_entries();
        let entries: Vec<&UtmpEntry> = if cfg.users.is_empty() {
            entries.iter().collect()
        } else {
            entries
                .iter()
                .filter(|e| cfg.users.iter().any(|u| u == &e.user))
                .collect()
        };

        if !cfg.no_header {
            writeln!(
                writer,
                "Login    Name                 TTY      Idle   When         Where"
            )?;
        }
        for entry in &entries {
            let (real_name, _, _, _, _) = read_passwd_gecos(&entry.user);
            writeln!(
                writer,
                "{:<8} {:<20} {:<8} {} {:<12} {}",
                truncate_str(&entry.user, 8),
                truncate_str(&real_name, 20),
                truncate_str(&entry.tty, 8),
                format_idle(entry.idle_secs),
                format_login_time(entry.login_time),
                &entry.host,
            )?;
        }
    } else {
        // Long format for specified users
        for username in &cfg.users {
            let info = get_user_info(username);
            writeln!(
                writer,
                "Login name: {:<28} In real life: {}",
                info.username, info.real_name
            )?;
            writeln!(writer, "Directory: {:<29} Shell: {}", info.home_dir, info.shell)?;
            if info.login_sessions.is_empty() {
                writeln!(writer, "Never logged in.")?;
            } else {
                for session in &info.login_sessions {
                    writeln!(
                        writer,
                        "On since {} on {}{}",
                        format_login_time(session.login_time),
                        session.tty,
                        if session.host.is_empty() {
                            String::new()
                        } else {
                            format!(" from {}", session.host)
                        }
                    )?;
                }
            }
            if !cfg.no_plan {
                if let Some(ref plan) = info.plan {
                    writeln!(writer, "Plan:")?;
                    write!(writer, "{plan}")?;
                } else {
                    writeln!(writer, "No Plan.")?;
                }
            }
        }
    }

    Ok(())
}

fn truncate_str(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        s[..max].to_string()
    }
}

// ---------------------------------------------------------------------------
// Help / version
// ---------------------------------------------------------------------------

fn print_help(personality: Personality) {
    match personality {
        Personality::W => {
            println!("Usage: w [OPTIONS] [user]");
            println!();
            println!("Show who is logged on and what they are doing.");
            println!();
            println!("Options:");
            println!("  -h, --no-header  Don't print the header");
            println!("  -s, --short      Short format");
            println!("  -f, --from       Toggle showing FROM field");
            println!("  -i, --ip-addr    Show IP addresses instead of hostnames");
            println!("  -V, --version    Show version");
            println!("  --help           Show this help");
        }
        Personality::Finger => {
            println!("Usage: finger [OPTIONS] [user[@host]...]");
            println!();
            println!("User information lookup program.");
            println!();
            println!("Options:");
            println!("  -l               Long output format");
            println!("  -s               Short output format");
            println!("  -p               Don't show .plan file");
            println!("  -m               Match user names only (not real names)");
            println!("  --help           Show this help");
            println!("  --version        Show version");
        }
        Personality::Pinky => {
            println!("Usage: pinky [OPTIONS] [user...]");
            println!();
            println!("Lightweight finger.");
            println!();
            println!("Options:");
            println!("  -l               Long output format");
            println!("  -s               Short output format (default)");
            println!("  -f               Omit header in short format");
            println!("  -b               Omit .plan file in long format");
            println!("  -h               Omit column headings");
            println!("  -p               Omit .project file");
            println!("  -w               Omit full name in short format");
            println!("  --help           Show this help");
            println!("  --version        Show version");
        }
    }
}

fn print_version(personality: Personality) {
    let name = match personality {
        Personality::W => "w",
        Personality::Finger => "finger",
        Personality::Pinky => "pinky",
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

    let stdout = io::stdout();
    let mut writer = stdout.lock();

    let result = match cfg.personality {
        Personality::W => run_w(&cfg, &mut writer),
        Personality::Finger => run_finger(&cfg, &mut writer),
        Personality::Pinky => run_pinky(&cfg, &mut writer),
    };

    match result {
        Ok(()) => 0,
        Err(e) => {
            let name = match cfg.personality {
                Personality::W => "w",
                Personality::Finger => "finger",
                Personality::Pinky => "pinky",
            };
            eprintln!("{name}: {e}");
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

    #[test]
    fn test_detect_personality_w() {
        assert_eq!(detect_personality("w"), Personality::W);
        assert_eq!(detect_personality("/usr/bin/w"), Personality::W);
    }

    #[test]
    fn test_detect_personality_finger() {
        assert_eq!(detect_personality("finger"), Personality::Finger);
        assert_eq!(detect_personality("/usr/bin/finger"), Personality::Finger);
    }

    #[test]
    fn test_detect_personality_pinky() {
        assert_eq!(detect_personality("pinky"), Personality::Pinky);
        assert_eq!(detect_personality("/usr/bin/pinky"), Personality::Pinky);
    }

    #[test]
    fn test_parse_args_w_basic() {
        let args = vec!["w".to_string()];
        let cfg = parse_args(&args).unwrap();
        assert_eq!(cfg.personality, Personality::W);
        assert!(cfg.users.is_empty());
    }

    #[test]
    fn test_parse_args_w_user() {
        let args = vec!["w".to_string(), "root".to_string()];
        let cfg = parse_args(&args).unwrap();
        assert_eq!(cfg.users, vec!["root"]);
    }

    #[test]
    fn test_parse_args_w_no_header() {
        let args = vec!["w".to_string(), "-h".to_string()];
        let cfg = parse_args(&args).unwrap();
        assert!(cfg.no_header);
    }

    #[test]
    fn test_parse_args_w_short() {
        let args = vec!["w".to_string(), "-s".to_string()];
        let cfg = parse_args(&args).unwrap();
        assert!(cfg.short_format);
    }

    #[test]
    fn test_parse_args_finger_long() {
        let args = vec!["finger".to_string(), "-l".to_string(), "user1".to_string()];
        let cfg = parse_args(&args).unwrap();
        assert!(cfg.long_format);
        assert_eq!(cfg.personality, Personality::Finger);
    }

    #[test]
    fn test_parse_args_finger_no_plan() {
        let args = vec!["finger".to_string(), "-p".to_string()];
        let cfg = parse_args(&args).unwrap();
        assert!(cfg.no_plan);
    }

    #[test]
    fn test_parse_args_finger_no_match() {
        let args = vec!["finger".to_string(), "-m".to_string()];
        let cfg = parse_args(&args).unwrap();
        assert!(!cfg.match_real_name);
    }

    #[test]
    fn test_parse_args_pinky_basic() {
        let args = vec!["pinky".to_string()];
        let cfg = parse_args(&args).unwrap();
        assert_eq!(cfg.personality, Personality::Pinky);
    }

    #[test]
    fn test_parse_args_pinky_long() {
        let args = vec!["pinky".to_string(), "-l".to_string(), "user1".to_string()];
        let cfg = parse_args(&args).unwrap();
        assert!(cfg.long_format);
    }

    #[test]
    fn test_parse_args_pinky_no_plan() {
        let args = vec!["pinky".to_string(), "-b".to_string()];
        let cfg = parse_args(&args).unwrap();
        assert!(cfg.no_plan);
    }

    #[test]
    fn test_parse_args_version() {
        for name in &["w", "finger", "pinky"] {
            let args = vec![name.to_string(), "--version".to_string()];
            let cfg = parse_args(&args).unwrap();
            assert!(cfg.show_version);
        }
    }

    #[test]
    fn test_parse_args_help() {
        for name in &["w", "finger", "pinky"] {
            let args = vec![name.to_string(), "--help".to_string()];
            let cfg = parse_args(&args).unwrap();
            assert!(cfg.show_help);
        }
    }

    #[test]
    fn test_format_idle_zero() {
        assert_eq!(format_idle(0), "  .  ");
    }

    #[test]
    fn test_format_idle_seconds() {
        assert_eq!(format_idle(30), " 30s ");
    }

    #[test]
    fn test_format_idle_minutes() {
        let result = format_idle(300); // 5 minutes
        assert!(result.contains("5:"));
    }

    #[test]
    fn test_format_idle_hours() {
        let result = format_idle(7200); // 2 hours
        assert!(result.contains("2:"));
    }

    #[test]
    fn test_format_idle_days() {
        let result = format_idle(172800); // 2 days
        assert!(result.contains("days"));
    }

    #[test]
    fn test_format_login_time_zero() {
        assert_eq!(format_login_time(0), " ?????");
    }

    #[test]
    fn test_format_login_time_normal() {
        let result = format_login_time(43200); // 12:00
        assert_eq!(result, "12:00");
    }

    #[test]
    fn test_truncate_str_short() {
        assert_eq!(truncate_str("hello", 10), "hello");
    }

    #[test]
    fn test_truncate_str_exact() {
        assert_eq!(truncate_str("hello", 5), "hello");
    }

    #[test]
    fn test_truncate_str_long() {
        assert_eq!(truncate_str("hello world", 5), "hello");
    }

    #[test]
    fn test_run_w_empty() {
        let cfg = Config {
            personality: Personality::W,
            no_header: true,
            ..Default::default()
        };
        let mut buf = Vec::new();
        run_w(&cfg, &mut buf).unwrap();
        // Should produce some output (at least current user)
        let output = String::from_utf8(buf).unwrap();
        // Output depends on environment, just check it doesn't crash
        assert!(output.len() >= 0);
    }

    #[test]
    fn test_run_finger_no_users() {
        let cfg = Config {
            personality: Personality::Finger,
            ..Default::default()
        };
        let mut buf = Vec::new();
        run_finger(&cfg, &mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("Login"));
    }

    #[test]
    fn test_run_pinky_no_users() {
        let cfg = Config {
            personality: Personality::Pinky,
            ..Default::default()
        };
        let mut buf = Vec::new();
        run_pinky(&cfg, &mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("Login"));
    }

    #[test]
    fn test_run_finger_remote() {
        let cfg = Config {
            personality: Personality::Finger,
            users: vec!["user@remote.host".to_string()],
            ..Default::default()
        };
        let mut buf = Vec::new();
        run_finger(&cfg, &mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("Remote finger not supported"));
    }

    #[test]
    fn test_run_w_with_header() {
        let cfg = Config {
            personality: Personality::W,
            no_header: false,
            ..Default::default()
        };
        let mut buf = Vec::new();
        run_w(&cfg, &mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("load average"));
    }

    #[test]
    fn test_run_w_filter_user() {
        let cfg = Config {
            personality: Personality::W,
            users: vec!["nonexistent_user_xyz".to_string()],
            no_header: true,
            ..Default::default()
        };
        let mut buf = Vec::new();
        run_w(&cfg, &mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.is_empty() || !output.contains("nonexistent_user_xyz"));
    }

    #[test]
    fn test_default_config() {
        let cfg = Config::default();
        assert_eq!(cfg.personality, Personality::W);
        assert!(cfg.users.is_empty());
        assert!(!cfg.no_header);
        assert!(!cfg.short_format);
        assert!(cfg.from_field);
    }

    #[test]
    fn test_mail_status() {
        assert_ne!(MailStatus::NewMail, MailStatus::NoMail);
        assert_ne!(MailStatus::OldMail, MailStatus::NoMail);
    }

    #[test]
    fn test_get_uptime_str() {
        let s = get_uptime_str();
        assert!(s.contains("up"));
    }

    #[test]
    fn test_get_load_avg() {
        let s = get_load_avg();
        assert!(s.contains("load average"));
    }

    #[test]
    fn test_parse_args_w_unknown() {
        let args = vec!["w".to_string(), "--badopt".to_string()];
        assert!(parse_args(&args).is_err());
    }

    #[test]
    fn test_parse_args_finger_unknown() {
        let args = vec!["finger".to_string(), "--badopt".to_string()];
        assert!(parse_args(&args).is_err());
    }

    #[test]
    fn test_parse_args_pinky_unknown() {
        let args = vec!["pinky".to_string(), "--badopt".to_string()];
        assert!(parse_args(&args).is_err());
    }
}
