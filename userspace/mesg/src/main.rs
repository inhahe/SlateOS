// OurOS mesg — control and send terminal messages
//
// Multi-personality binary:
//   mesg   — control write access to your terminal
//   write  — send a message to another user's terminal
//   talk   — talk to another user (split-screen conversation)
//
// Usage:
//   mesg [y|n]
//   write <user> [ttyname]
//   talk <user> [ttyname]

#![cfg_attr(not(test), no_main)]

use std::env;
use std::io::{self, BufRead, Write};
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// Personality detection
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Personality {
    Mesg,
    WriteMsg,
    Talk,
}

fn detect_personality(argv0: &str) -> Personality {
    let base = argv0.rsplit('/').next().unwrap_or(argv0);
    let base = base.rsplit('\\').next().unwrap_or(base);
    let lower = base.to_ascii_lowercase();
    let lower = lower.strip_suffix(".exe").unwrap_or(&lower);
    match lower {
        "write" => Personality::WriteMsg,
        "talk" => Personality::Talk,
        _ => Personality::Mesg,
    }
}

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct Config {
    personality: Personality,
    // mesg
    mesg_arg: Option<MesgAction>,
    // write/talk
    target_user: Option<String>,
    target_tty: Option<String>,
    show_help: bool,
    show_version: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MesgAction {
    Allow,   // y
    Deny,    // n
    Status,  // no arg — show current status
}

impl Default for Config {
    fn default() -> Self {
        Self {
            personality: Personality::Mesg,
            mesg_arg: None,
            target_user: None,
            target_tty: None,
            show_help: false,
            show_version: false,
        }
    }
}

fn parse_args(args: &[String]) -> Result<Config, String> {
    let personality = args
        .first()
        .map(|a| detect_personality(a))
        .unwrap_or(Personality::Mesg);

    let mut cfg = Config {
        personality,
        ..Default::default()
    };

    let mut positional = Vec::new();
    let mut i = 1;

    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            "-h" | "--help" => cfg.show_help = true,
            "-V" | "--version" => cfg.show_version = true,
            other if other.starts_with('-') && other.len() > 1 => {
                // Might be a flag, or might be -v for verbose in some tools
                match personality {
                    Personality::Mesg => {
                        return Err(format!("mesg: unknown option: {other}"));
                    }
                    Personality::WriteMsg => {
                        return Err(format!("write: unknown option: {other}"));
                    }
                    Personality::Talk => {
                        return Err(format!("talk: unknown option: {other}"));
                    }
                }
            }
            _ => positional.push(arg.clone()),
        }
        i += 1;
    }

    match personality {
        Personality::Mesg => {
            if let Some(action) = positional.first() {
                cfg.mesg_arg = Some(match action.as_str() {
                    "y" | "yes" => MesgAction::Allow,
                    "n" | "no" => MesgAction::Deny,
                    other => return Err(format!("mesg: invalid argument: {other} (use y or n)")),
                });
            } else {
                cfg.mesg_arg = Some(MesgAction::Status);
            }
        }
        Personality::WriteMsg | Personality::Talk => {
            if positional.is_empty() && !cfg.show_help && !cfg.show_version {
                let name = if personality == Personality::WriteMsg {
                    "write"
                } else {
                    "talk"
                };
                return Err(format!("{name}: missing user operand"));
            }
            cfg.target_user = positional.first().cloned();
            cfg.target_tty = positional.get(1).cloned();
        }
    }

    Ok(cfg)
}

// ---------------------------------------------------------------------------
// Mesg implementation
// ---------------------------------------------------------------------------

/// Get the current terminal device path. Kept for the future code path
/// that will switch the state file to a per-tty location; the current
/// implementation uses a single shared state file.
#[allow(dead_code)]
fn get_tty() -> Option<PathBuf> {
    // Try standard env var first
    if let Ok(tty) = env::var("TTY") {
        return Some(PathBuf::from(tty));
    }

    // Try /dev/tty
    if PathBuf::from("/dev/tty").exists() {
        return Some(PathBuf::from("/dev/tty"));
    }

    None
}

/// Check if the current terminal allows messages
fn get_mesg_status() -> MesgState {
    // In a real OS, check the terminal's group-write permission bit
    // For now, read from a state file
    let state_file = get_mesg_state_path();
    if let Ok(content) = std::fs::read_to_string(&state_file) {
        match content.trim() {
            "n" => return MesgState::No,
            "y" => return MesgState::Yes,
            _ => {}
        }
    }
    MesgState::Yes // default: allow
}

fn set_mesg_status(allow: bool) -> io::Result<()> {
    let state_file = get_mesg_state_path();
    // Ensure parent directory exists
    if let Some(parent) = state_file.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::write(&state_file, if allow { "y\n" } else { "n\n" })
}

fn get_mesg_state_path() -> PathBuf {
    let user = env::var("USER").unwrap_or_else(|_| "unknown".to_string());
    PathBuf::from(format!("/var/run/mesg/{user}"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MesgState {
    Yes,
    No,
}

fn run_mesg(action: MesgAction, writer: &mut dyn Write) -> Result<i32, io::Error> {
    match action {
        MesgAction::Status => {
            let status = get_mesg_status();
            match status {
                MesgState::Yes => {
                    writeln!(writer, "is y")?;
                    Ok(0)
                }
                MesgState::No => {
                    writeln!(writer, "is n")?;
                    Ok(1)
                }
            }
        }
        MesgAction::Allow => {
            set_mesg_status(true)?;
            Ok(0)
        }
        MesgAction::Deny => {
            set_mesg_status(false)?;
            Ok(0)
        }
    }
}

// ---------------------------------------------------------------------------
// Write implementation
// ---------------------------------------------------------------------------

/// Find the target user's terminal
fn find_user_tty(username: &str, tty_hint: Option<&str>) -> Option<PathBuf> {
    // If a specific tty is given, use it
    if let Some(tty) = tty_hint {
        let path = if tty.starts_with("/dev/") {
            PathBuf::from(tty)
        } else {
            PathBuf::from(format!("/dev/{tty}"))
        };
        return Some(path);
    }

    // Search utmp for the user's tty
    if let Ok(content) = std::fs::read_to_string("/var/run/utmp") {
        for line in content.lines() {
            let fields: Vec<&str> = line.split(':').collect();
            if fields.len() >= 2 && fields[0] == username {
                let tty = fields[1];
                let path = if tty.starts_with("/dev/") {
                    PathBuf::from(tty)
                } else {
                    PathBuf::from(format!("/dev/{tty}"))
                };
                return Some(path);
            }
        }
    }

    None
}

/// Check if the target user has messages enabled
fn check_user_mesg(username: &str) -> bool {
    let state_file = PathBuf::from(format!("/var/run/mesg/{username}"));
    if let Ok(content) = std::fs::read_to_string(state_file) {
        return content.trim() != "n";
    }
    true // default: allow
}

fn run_write(
    target_user: &str,
    target_tty: Option<&str>,
    reader: &mut dyn BufRead,
    writer: &mut dyn Write,
    err_writer: &mut dyn Write,
) -> Result<i32, io::Error> {
    let from_user = env::var("USER").unwrap_or_else(|_| "unknown".to_string());
    let from_tty = env::var("TTY").unwrap_or_else(|_| "?".to_string());

    // Check if target user accepts messages
    if !check_user_mesg(target_user) {
        writeln!(
            err_writer,
            "write: {target_user} has messages disabled"
        )?;
        return Ok(1);
    }

    // Find target terminal
    let tty_path = match find_user_tty(target_user, target_tty) {
        Some(p) => p,
        None => {
            writeln!(err_writer, "write: {target_user} is not logged in")?;
            if let Some(tty) = target_tty {
                writeln!(err_writer, "write: {target_user} is not logged in on {tty}")?;
            }
            return Ok(1);
        }
    };

    // In a real OS, we'd open the tty device and write to it
    // For now, we'll simulate by writing to a message file
    let msg_file = PathBuf::from(format!("/var/run/messages/{target_user}"));

    // Header
    let header = format!(
        "\r\nMessage from {from_user}@{from_tty} on {} ...\r\n",
        tty_path.display()
    );

    let mut messages = Vec::new();
    messages.extend_from_slice(header.as_bytes());

    // Read and forward lines
    writeln!(writer, "Message to {target_user} (Ctrl-D to end):")?;
    writer.flush()?;

    let mut line = String::new();
    loop {
        line.clear();
        let n = reader.read_line(&mut line)?;
        if n == 0 {
            break; // EOF
        }

        messages.extend_from_slice(line.as_bytes());
    }

    messages.extend_from_slice(b"\r\nEOF\r\n");

    // Write to message file (in real OS: write to tty device)
    if let Some(parent) = msg_file.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&msg_file)
        .and_then(|mut f| f.write_all(&messages));

    Ok(0)
}

// ---------------------------------------------------------------------------
// Talk implementation
// ---------------------------------------------------------------------------

fn run_talk(
    target_user: &str,
    target_tty: Option<&str>,
    reader: &mut dyn BufRead,
    writer: &mut dyn Write,
    err_writer: &mut dyn Write,
) -> Result<i32, io::Error> {
    let from_user = env::var("USER").unwrap_or_else(|_| "unknown".to_string());

    // Check if target user is available
    if !check_user_mesg(target_user) {
        writeln!(
            err_writer,
            "talk: {target_user} has messages disabled"
        )?;
        return Ok(1);
    }

    let _tty_path = match find_user_tty(target_user, target_tty) {
        Some(p) => p,
        None => {
            writeln!(err_writer, "talk: {target_user} is not logged in")?;
            return Ok(1);
        }
    };

    // In a real OS, talk uses a split-screen curses UI with a talk daemon
    // For a simplified version, we just relay messages like write

    writeln!(writer, "[Connecting to {target_user}...]")?;
    writeln!(writer, "[Talk mode - type Ctrl-D to quit]")?;
    writeln!(writer, "[Connection from {from_user}]")?;
    writer.flush()?;

    let msg_file = PathBuf::from(format!("/var/run/talk/{from_user}-{target_user}"));
    if let Some(parent) = msg_file.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let mut line = String::new();
    loop {
        line.clear();
        let n = reader.read_line(&mut line)?;
        if n == 0 {
            break;
        }

        // Echo to output (in real talk, this goes to the remote terminal too)
        write!(writer, "{line}")?;
        writer.flush()?;

        // Append to talk log
        let _ = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&msg_file)
            .and_then(|mut f| f.write_all(line.as_bytes()));
    }

    writeln!(writer, "[Connection closed]")?;

    Ok(0)
}

// ---------------------------------------------------------------------------
// Help / version
// ---------------------------------------------------------------------------

#[cfg(not(test))]
fn print_help(personality: Personality) {
    match personality {
        Personality::Mesg => {
            println!("Usage: mesg [y|n]");
            println!();
            println!("Control write access to your terminal.");
            println!();
            println!("  y     Allow messages");
            println!("  n     Deny messages");
            println!("  (none) Show current setting");
            println!();
            println!("Exit status: 0 if messages are allowed, 1 if denied.");
        }
        Personality::WriteMsg => {
            println!("Usage: write <user> [tty]");
            println!();
            println!("Send a message to another user's terminal.");
            println!();
            println!("Type your message then press Ctrl-D to send.");
            println!("The recipient will see a header identifying the sender.");
        }
        Personality::Talk => {
            println!("Usage: talk <user> [tty]");
            println!();
            println!("Talk to another user in a split-screen conversation.");
            println!();
            println!("Establishes a two-way text connection with the named user.");
            println!("Press Ctrl-D to end the conversation.");
        }
    }
}

#[cfg(not(test))]
fn print_version(personality: Personality) {
    let name = match personality {
        Personality::Mesg => "mesg",
        Personality::WriteMsg => "write",
        Personality::Talk => "talk",
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
        Personality::Mesg => {
            let action = cfg.mesg_arg.unwrap_or(MesgAction::Status);
            match run_mesg(action, &mut writer) {
                Ok(code) => code,
                Err(e) => {
                    eprintln!("mesg: {e}");
                    2
                }
            }
        }
        Personality::WriteMsg => {
            let target = cfg.target_user.as_deref().unwrap_or("");
            match run_write(
                target,
                cfg.target_tty.as_deref(),
                &mut reader,
                &mut writer,
                &mut err_writer,
            ) {
                Ok(code) => code,
                Err(e) => {
                    eprintln!("write: {e}");
                    1
                }
            }
        }
        Personality::Talk => {
            let target = cfg.target_user.as_deref().unwrap_or("");
            match run_talk(
                target,
                cfg.target_tty.as_deref(),
                &mut reader,
                &mut writer,
                &mut err_writer,
            ) {
                Ok(code) => code,
                Err(e) => {
                    eprintln!("talk: {e}");
                    1
                }
            }
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
    fn test_detect_personality_mesg() {
        assert_eq!(detect_personality("mesg"), Personality::Mesg);
        assert_eq!(detect_personality("/usr/bin/mesg"), Personality::Mesg);
    }

    #[test]
    fn test_detect_personality_write() {
        assert_eq!(detect_personality("write"), Personality::WriteMsg);
        assert_eq!(detect_personality("/usr/bin/write"), Personality::WriteMsg);
    }

    #[test]
    fn test_detect_personality_talk() {
        assert_eq!(detect_personality("talk"), Personality::Talk);
        assert_eq!(detect_personality("/usr/bin/talk"), Personality::Talk);
    }

    #[test]
    fn test_parse_args_mesg_status() {
        let args = vec!["mesg".to_string()];
        let cfg = parse_args(&args).unwrap();
        assert_eq!(cfg.personality, Personality::Mesg);
        assert_eq!(cfg.mesg_arg, Some(MesgAction::Status));
    }

    #[test]
    fn test_parse_args_mesg_y() {
        let args = vec!["mesg".to_string(), "y".to_string()];
        let cfg = parse_args(&args).unwrap();
        assert_eq!(cfg.mesg_arg, Some(MesgAction::Allow));
    }

    #[test]
    fn test_parse_args_mesg_n() {
        let args = vec!["mesg".to_string(), "n".to_string()];
        let cfg = parse_args(&args).unwrap();
        assert_eq!(cfg.mesg_arg, Some(MesgAction::Deny));
    }

    #[test]
    fn test_parse_args_mesg_yes() {
        let args = vec!["mesg".to_string(), "yes".to_string()];
        let cfg = parse_args(&args).unwrap();
        assert_eq!(cfg.mesg_arg, Some(MesgAction::Allow));
    }

    #[test]
    fn test_parse_args_mesg_no() {
        let args = vec!["mesg".to_string(), "no".to_string()];
        let cfg = parse_args(&args).unwrap();
        assert_eq!(cfg.mesg_arg, Some(MesgAction::Deny));
    }

    #[test]
    fn test_parse_args_mesg_invalid() {
        let args = vec!["mesg".to_string(), "x".to_string()];
        assert!(parse_args(&args).is_err());
    }

    #[test]
    fn test_parse_args_write_user() {
        let args = vec!["write".to_string(), "john".to_string()];
        let cfg = parse_args(&args).unwrap();
        assert_eq!(cfg.personality, Personality::WriteMsg);
        assert_eq!(cfg.target_user, Some("john".to_string()));
    }

    #[test]
    fn test_parse_args_write_user_tty() {
        let args = vec![
            "write".to_string(),
            "john".to_string(),
            "tty1".to_string(),
        ];
        let cfg = parse_args(&args).unwrap();
        assert_eq!(cfg.target_user, Some("john".to_string()));
        assert_eq!(cfg.target_tty, Some("tty1".to_string()));
    }

    #[test]
    fn test_parse_args_write_no_user() {
        let args = vec!["write".to_string()];
        assert!(parse_args(&args).is_err());
    }

    #[test]
    fn test_parse_args_talk_user() {
        let args = vec!["talk".to_string(), "bob".to_string()];
        let cfg = parse_args(&args).unwrap();
        assert_eq!(cfg.personality, Personality::Talk);
        assert_eq!(cfg.target_user, Some("bob".to_string()));
    }

    #[test]
    fn test_parse_args_talk_no_user() {
        let args = vec!["talk".to_string()];
        assert!(parse_args(&args).is_err());
    }

    #[test]
    fn test_parse_args_help() {
        for name in &["mesg", "write", "talk"] {
            let args = vec![name.to_string(), "--help".to_string()];
            let cfg = parse_args(&args).unwrap();
            assert!(cfg.show_help);
        }
    }

    #[test]
    fn test_parse_args_version() {
        for name in &["mesg", "write", "talk"] {
            let args = vec![name.to_string(), "--version".to_string()];
            let cfg = parse_args(&args).unwrap();
            assert!(cfg.show_version);
        }
    }

    #[test]
    fn test_run_mesg_status() {
        let mut buf = Vec::new();
        let code = run_mesg(MesgAction::Status, &mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("is y") || output.contains("is n"));
        assert!(code == 0 || code == 1);
    }

    #[test]
    fn test_run_write_user_not_logged_in() {
        let input = b"Hello\n";
        let mut reader = Cursor::new(input.as_slice());
        let mut writer = Vec::new();
        let mut err_writer = Vec::new();
        let code = run_write(
            "nonexistent_user_xyz",
            None,
            &mut reader,
            &mut writer,
            &mut err_writer,
        )
        .unwrap();
        assert_eq!(code, 1);
        let err = String::from_utf8(err_writer).unwrap();
        assert!(err.contains("not logged in"));
    }

    #[test]
    fn test_run_talk_user_not_logged_in() {
        let input = b"Hello\n";
        let mut reader = Cursor::new(input.as_slice());
        let mut writer = Vec::new();
        let mut err_writer = Vec::new();
        let code = run_talk(
            "nonexistent_user_xyz",
            None,
            &mut reader,
            &mut writer,
            &mut err_writer,
        )
        .unwrap();
        assert_eq!(code, 1);
    }

    #[test]
    fn test_find_user_tty_with_hint() {
        let result = find_user_tty("anyone", Some("tty1"));
        assert_eq!(result, Some(PathBuf::from("/dev/tty1")));
    }

    #[test]
    fn test_find_user_tty_absolute_hint() {
        let result = find_user_tty("anyone", Some("/dev/pts/0"));
        assert_eq!(result, Some(PathBuf::from("/dev/pts/0")));
    }

    #[test]
    fn test_check_user_mesg_default() {
        // For a nonexistent user, default is allow
        assert!(check_user_mesg("nonexistent_user_xyz"));
    }

    #[test]
    fn test_mesg_state() {
        assert_eq!(MesgState::Yes, MesgState::Yes);
        assert_ne!(MesgState::Yes, MesgState::No);
    }

    #[test]
    fn test_default_config() {
        let cfg = Config::default();
        assert_eq!(cfg.personality, Personality::Mesg);
        assert_eq!(cfg.target_user, None);
        assert_eq!(cfg.target_tty, None);
        assert!(!cfg.show_help);
    }

    #[test]
    fn test_parse_args_mesg_unknown_flag() {
        let args = vec!["mesg".to_string(), "-x".to_string()];
        assert!(parse_args(&args).is_err());
    }

    #[test]
    fn test_parse_args_write_unknown_flag() {
        let args = vec!["write".to_string(), "-x".to_string()];
        assert!(parse_args(&args).is_err());
    }
}
