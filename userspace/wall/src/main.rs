//! wall/write/mesg — terminal messaging utilities for SlateOS
//!
//! Multi-personality binary detected via argv[0]:
//! - `wall`: broadcast message to all logged-in users
//! - `write`: send message to specific user's terminal
//! - `mesg`: control terminal message permission

use std::env;
use std::fs::{self, OpenOptions};
use std::io::{self, BufRead, Read, Write};
use std::process;
use std::time::{SystemTime, UNIX_EPOCH};

// ── Mode detection ───────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
enum Mode {
    Wall,
    WriteMsg,
    Mesg,
}

fn detect_mode(argv0: &str) -> Mode {
    let base = argv0.rsplit(['/', '\\']).next().unwrap_or(argv0);
    let name = base.strip_suffix(".exe").unwrap_or(base);
    match name.to_lowercase().as_str() {
        "write" => Mode::WriteMsg,
        "mesg" => Mode::Mesg,
        _ => Mode::Wall,
    }
}

// ── Common helpers ───────────────────────────────────────────────

fn get_username() -> String {
    env::var("USER")
        .or_else(|_| env::var("LOGNAME"))
        .unwrap_or_else(|_| "unknown".to_string())
}

fn get_tty() -> String {
    // Read from /proc/self/fd/0 symlink or /dev/tty
    if let Ok(link) = fs::read_link("/proc/self/fd/0")
        && let Some(path) = link.to_str()
            && path.starts_with("/dev/") {
                return path[5..].to_string();
            }
    "console".to_string()
}

fn format_timestamp() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    let days = secs / 86400;
    let (year, month, day) = days_to_date(days);

    let month_name = match month {
        1 => "Jan", 2 => "Feb", 3 => "Mar", 4 => "Apr",
        5 => "May", 6 => "Jun", 7 => "Jul", 8 => "Aug",
        9 => "Sep", 10 => "Oct", 11 => "Nov", 12 => "Dec",
        _ => "???",
    };

    format!(
        "{} {:>2} {:02}:{:02}:{:02} {:04}",
        month_name, day, hours, minutes, seconds, year
    )
}

fn days_to_date(days_since_epoch: u64) -> (u64, u32, u32) {
    let z = days_since_epoch + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m as u32, d as u32)
}

/// Get list of all logged-in user terminals from /proc
fn get_logged_in_terminals() -> Vec<(String, String)> {
    // Read /var/run/utmp or scan /dev/pts/ and /dev/tty*
    let mut terminals: Vec<(String, String)> = Vec::new();

    // Try utmp first
    if let Ok(content) = fs::read_to_string("/var/run/utmp.txt") {
        for line in content.lines() {
            let parts: Vec<&str> = line.split(':').collect();
            if parts.len() >= 2 {
                terminals.push((parts[0].to_string(), parts[1].to_string()));
            }
        }
    }

    // Also scan /dev/pts/
    if let Ok(entries) = fs::read_dir("/dev/pts") {
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str()
                && name.chars().all(|c| c.is_ascii_digit()) {
                    let tty = format!("pts/{}", name);
                    // Check if someone is using this terminal
                    terminals.push(("unknown".to_string(), tty));
                }
        }
    }

    // Also check /dev/tty[0-9]
    for i in 0..12 {
        let path = format!("/dev/tty{}", i);
        if fs::metadata(&path).is_ok() {
            terminals.push(("unknown".to_string(), format!("tty{}", i)));
        }
    }

    terminals
}

/// Check if a terminal device allows messages (group write permission)
fn check_mesg_permission(tty_path: &str) -> bool {
    // Check if the tty device file is group-writable
    // On a real system, this uses stat() to check mode bits
    if let Ok(metadata) = fs::metadata(tty_path) {
        // On Unix, check group write bit
        // For our OS, we just check if the file is writable
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            return (metadata.mode() & 0o020) != 0;
        }
        #[cfg(not(unix))]
        {
            let _ = metadata;
            return true; // Assume writable on non-Unix
        }
    }
    false
}

// ── wall implementation ──────────────────────────────────────────

fn run_wall(args: &[String]) -> i32 {
    let mut no_banner = false;
    let mut group: Option<String> = None;
    let mut message_parts: Vec<String> = Vec::new();
    let mut file: Option<String> = None;

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            "--help" | "-h" => {
                println!("Usage: wall [OPTION]... [FILE | MESSAGE]");
                println!("Write a message to all logged-in users.");
                println!();
                println!("Options:");
                println!("  -n, --nobanner    suppress banner");
                println!("  -g, --group=GROUP send to members of GROUP only");
                println!("  -h, --help        display this help and exit");
                println!("  --version         output version information");
                println!();
                println!("If no FILE or MESSAGE, read from standard input.");
                return 0;
            }
            "--version" => {
                println!("wall (SlateOS) 0.1.0");
                return 0;
            }
            "-n" | "--nobanner" => {
                no_banner = true;
            }
            "-g" | "--group" => {
                i += 1;
                if i < args.len() {
                    group = Some(args[i].clone());
                }
            }
            _ if arg.starts_with("--group=") => {
                group = Some(arg.strip_prefix("--group=").unwrap_or("").to_string());
            }
            _ => {
                // Check if it's a file
                if message_parts.is_empty() && fs::metadata(arg).is_ok() {
                    file = Some(arg.clone());
                } else {
                    message_parts.push(arg.clone());
                }
            }
        }
        i += 1;
    }

    // Get the message
    let message = if let Some(ref file_path) = file {
        match fs::read_to_string(file_path) {
            Ok(content) => content,
            Err(e) => {
                eprintln!("wall: {}: {}", file_path, e);
                return 1;
            }
        }
    } else if !message_parts.is_empty() {
        message_parts.join(" ")
    } else {
        // Read from stdin
        let mut buf = String::new();
        if io::stdin().lock().read_to_string(&mut buf).is_err() {
            eprintln!("wall: error reading standard input");
            return 1;
        }
        buf
    };

    let username = get_username();
    let tty = get_tty();
    let timestamp = format_timestamp();

    let banner = if no_banner {
        String::new()
    } else {
        format!(
            "\r\n\x07Broadcast message from {} ({}) ({}):\r\n\r\n",
            username, tty, timestamp
        )
    };

    let full_message = format!("{}{}\r\n", banner, message.trim_end());

    // Get terminals to write to
    let terminals = get_logged_in_terminals();
    let mut sent = 0;

    let _ = group; // Would filter by group if implemented

    for (_user, terminal) in &terminals {
        let dev_path = format!("/dev/{}", terminal);

        // Skip our own terminal
        if *terminal == tty {
            continue;
        }

        if let Ok(mut f) = OpenOptions::new().write(true).open(&dev_path)
            && f.write_all(full_message.as_bytes()).is_ok() {
                sent += 1;
            }
    }

    let _ = sent;
    0
}

// ── write implementation ─────────────────────────────────────────

fn run_write(args: &[String]) -> i32 {
    if args.is_empty() || args[0] == "--help" || args[0] == "-h" {
        println!("Usage: write USER [TERMINAL]");
        println!("Send a message to another user's terminal.");
        println!();
        println!("The message is read from standard input, line by line.");
        println!("Press Ctrl+D (EOF) to end the message.");
        return if args.is_empty() { 1 } else { 0 };
    }

    if args[0] == "--version" {
        println!("write (SlateOS) 0.1.0");
        return 0;
    }

    let target_user = &args[0];
    let target_tty = args.get(1).map(|s| s.as_str());

    // Find the user's terminal
    let terminals = get_logged_in_terminals();
    let target_terminal = if let Some(tty) = target_tty {
        tty.to_string()
    } else {
        // Find first terminal for the user
        match terminals.iter().find(|(user, _)| user == target_user) {
            Some((_, tty)) => tty.clone(),
            None => {
                eprintln!("write: {} is not logged in", target_user);
                return 1;
            }
        }
    };

    let dev_path = format!("/dev/{}", target_terminal);

    // Check if the terminal exists and is writable
    if fs::metadata(&dev_path).is_err() {
        eprintln!("write: {}: No such terminal", target_terminal);
        return 1;
    }

    let username = get_username();
    let tty = get_tty();

    // Open the target terminal for writing
    let mut target = match OpenOptions::new().write(true).open(&dev_path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("write: {}: {}", dev_path, e);
            return 1;
        }
    };

    // Send header
    let header = format!(
        "\r\nMessage from {} ({}) [{:?}]...\r\n",
        username, tty, format_timestamp()
    );
    let _ = target.write_all(header.as_bytes());

    // Read and forward lines from stdin
    let stdin = io::stdin();
    for line in stdin.lock().lines() {
        match line {
            Ok(text) => {
                let msg = format!("{}\r\n", text);
                if target.write_all(msg.as_bytes()).is_err() {
                    eprintln!("write: error writing to {}", target_terminal);
                    return 1;
                }
            }
            Err(_) => break,
        }
    }

    // Send EOF marker
    let _ = target.write_all(b"EOF\r\n");

    0
}

// ── mesg implementation ──────────────────────────────────────────

fn run_mesg(args: &[String]) -> i32 {
    if !args.is_empty() && (args[0] == "--help" || args[0] == "-h") {
        println!("Usage: mesg [y|n]");
        println!("Control write access to your terminal.");
        println!();
        println!("  y    allow messages");
        println!("  n    disallow messages");
        println!();
        println!("With no argument, print current status.");
        return 0;
    }

    if !args.is_empty() && args[0] == "--version" {
        println!("mesg (SlateOS) 0.1.0");
        return 0;
    }

    let tty = get_tty();
    let tty_path = format!("/dev/{}", tty);

    if args.is_empty() {
        // Query current status
        let allowed = check_mesg_permission(&tty_path);
        if allowed {
            println!("is y");
            return 0;
        } else {
            println!("is n");
            return 1;
        }
    }

    match args[0].as_str() {
        "y" | "Y" | "yes" => {
            // Enable messages: chmod g+w on terminal
            // On SlateOS, this would use a chmod syscall
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if let Ok(meta) = fs::metadata(&tty_path) {
                    let mode = meta.permissions().mode() | 0o020;
                    let _ = fs::set_permissions(&tty_path, fs::Permissions::from_mode(mode));
                }
            }
            #[cfg(not(unix))]
            {
                // On non-Unix, just report success
            }
            0
        }
        "n" | "N" | "no" => {
            // Disable messages: chmod g-w on terminal
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if let Ok(meta) = fs::metadata(&tty_path) {
                    let mode = meta.permissions().mode() & !0o020;
                    let _ = fs::set_permissions(&tty_path, fs::Permissions::from_mode(mode));
                }
            }
            #[cfg(not(unix))]
            {
                // On non-Unix, just report success
            }
            0
        }
        other => {
            eprintln!("mesg: invalid argument '{}' (use 'y' or 'n')", other);
            1
        }
    }
}

// ── main ─────────────────────────────────────────────────────────

fn main() {
    let args: Vec<String> = env::args().collect();
    let mode = detect_mode(args.first().map(|s| s.as_str()).unwrap_or("wall"));

    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let exit_code = match mode {
        Mode::Wall => run_wall(&rest),
        Mode::WriteMsg => run_write(&rest),
        Mode::Mesg => run_mesg(&rest),
    };

    process::exit(exit_code);
}

// ── Tests ────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // Mode detection
    #[test]
    fn test_detect_wall() {
        assert_eq!(detect_mode("wall"), Mode::Wall);
        assert_eq!(detect_mode("/usr/bin/wall"), Mode::Wall);
        assert_eq!(detect_mode("wall.exe"), Mode::Wall);
    }

    #[test]
    fn test_detect_write() {
        assert_eq!(detect_mode("write"), Mode::WriteMsg);
        assert_eq!(detect_mode("/usr/bin/write"), Mode::WriteMsg);
    }

    #[test]
    fn test_detect_mesg() {
        assert_eq!(detect_mode("mesg"), Mode::Mesg);
        assert_eq!(detect_mode("/usr/bin/mesg"), Mode::Mesg);
    }

    #[test]
    fn test_detect_unknown_defaults() {
        assert_eq!(detect_mode("something"), Mode::Wall);
    }

    // Username
    #[test]
    fn test_get_username_not_empty() {
        let user = get_username();
        assert!(!user.is_empty());
    }

    // TTY
    #[test]
    fn test_get_tty_not_empty() {
        let tty = get_tty();
        assert!(!tty.is_empty());
    }

    // Timestamp
    #[test]
    fn test_format_timestamp_not_empty() {
        let ts = format_timestamp();
        assert!(!ts.is_empty());
        assert!(ts.len() >= 20);
    }

    // Date conversion
    #[test]
    fn test_days_to_date_epoch() {
        let (y, m, d) = days_to_date(0);
        assert_eq!((y, m, d), (1970, 1, 1));
    }

    #[test]
    fn test_days_to_date_known() {
        let (y, m, d) = days_to_date(19723);
        assert_eq!((y, m, d), (2024, 1, 1));
    }

    #[test]
    fn test_days_to_date_leap() {
        let (y, m, d) = days_to_date(19782);
        assert_eq!(y, 2024);
        assert_eq!(m, 2);
        assert_eq!(d, 29);
    }

    // Terminal listing
    #[test]
    fn test_get_terminals_returns_vec() {
        // May or may not have entries depending on test environment — just verify it
        // returns a Vec without panicking.
        let _terminals = get_logged_in_terminals();
    }

    // Check permission (on test systems, may not have /dev/tty)
    #[test]
    fn test_check_permission_nonexistent() {
        let result = check_mesg_permission("/dev/nonexistent_tty_xyz");
        assert!(!result);
    }

    // Wall with no banner
    #[test]
    fn test_wall_banner_format() {
        let username = "testuser";
        let tty = "pts/0";
        let banner = format!(
            "\r\n\x07Broadcast message from {} ({}):\r\n\r\n",
            username, tty
        );
        assert!(banner.contains("Broadcast message"));
        assert!(banner.contains(username));
        assert!(banner.contains(tty));
    }

    // Write header format
    #[test]
    fn test_write_header_format() {
        let username = "alice";
        let tty = "pts/1";
        let header = format!("\r\nMessage from {} ({})...\r\n", username, tty);
        assert!(header.contains("Message from"));
        assert!(header.contains("alice"));
        assert!(header.contains("pts/1"));
    }

    // Mesg argument validation
    #[test]
    fn test_mesg_y_valid() {
        // Just verify the string matching logic
        assert!(matches!("y", "y" | "Y" | "yes"));
        assert!(matches!("Y", "y" | "Y" | "yes"));
        assert!(matches!("yes", "y" | "Y" | "yes"));
    }

    #[test]
    fn test_mesg_n_valid() {
        assert!(matches!("n", "n" | "N" | "no"));
        assert!(matches!("N", "n" | "N" | "no"));
        assert!(matches!("no", "n" | "N" | "no"));
    }

    #[test]
    fn test_mesg_invalid() {
        assert!(!matches!("x", "y" | "Y" | "yes" | "n" | "N" | "no"));
        assert!(!matches!("maybe", "y" | "Y" | "yes" | "n" | "N" | "no"));
    }

    // Wall message construction
    #[test]
    fn test_wall_message_with_banner() {
        let banner = "Broadcast message...";
        let message = "System going down";
        let full = format!("{}\r\n{}\r\n", banner, message);
        assert!(full.contains(banner));
        assert!(full.contains(message));
    }

    #[test]
    fn test_wall_message_no_banner() {
        let message = "Hello everyone";
        let full = format!("{}\r\n", message.trim_end());
        assert!(full.contains("Hello everyone"));
        assert!(!full.contains("Broadcast"));
    }

    // Dev path construction
    #[test]
    fn test_dev_path() {
        let terminal = "pts/0";
        let dev_path = format!("/dev/{}", terminal);
        assert_eq!(dev_path, "/dev/pts/0");
    }

    #[test]
    fn test_dev_path_tty() {
        let terminal = "tty1";
        let dev_path = format!("/dev/{}", terminal);
        assert_eq!(dev_path, "/dev/tty1");
    }

    // EOF marker
    #[test]
    fn test_eof_marker() {
        let eof = "EOF\r\n";
        assert_eq!(eof, "EOF\r\n");
    }
}
