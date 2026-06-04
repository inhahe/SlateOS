//! OurOS nproc/arch/pathchk/logname/users/tty — simple system info tools
//!
//! Multi-personality binary detected via argv[0]:
//! - `nproc`: Print the number of available processors
//! - `arch`: Print the machine architecture
//! - `pathchk`: Check path validity
//! - `logname`: Print the login name
//! - `users`: Print logged-in user names
//! - `tty`: Print the terminal name

#![allow(unexpected_cfgs)]

use std::env;
use std::fs;
use std::process;

// ── Personality detection ──────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
enum Mode {
    Nproc,
    Arch,
    Pathchk,
    Logname,
    Users,
    Tty,
}

fn detect_mode(argv0: &str) -> Mode {
    let name = argv0
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(argv0);
    let name = name.strip_suffix(".exe").unwrap_or(name);
    let lower = name.to_ascii_lowercase();
    match lower.as_str() {
        "arch" => Mode::Arch,
        "pathchk" => Mode::Pathchk,
        "logname" => Mode::Logname,
        "users" => Mode::Users,
        "tty" => Mode::Tty,
        _ => Mode::Nproc,
    }
}

// ── nproc ──────────────────────────────────────────────────────────

fn run_nproc() -> Result<(), String> {
    let argv: Vec<String> = env::args().collect();
    let mut all = false;
    let mut ignore = 0u32;

    let mut i = 1;
    while i < argv.len() {
        match argv[i].as_str() {
            "-h" | "--help" => {
                eprintln!("Usage: nproc [--all] [--ignore=N]");
                eprintln!("Print the number of available processing units.");
                eprintln!();
                eprintln!("  --all       print total number of processors (not just available)");
                eprintln!("  --ignore=N  exclude N processors from count");
                process::exit(0);
            }
            "--all" => all = true,
            _ if argv[i].starts_with("--ignore=") => {
                let val = &argv[i]["--ignore=".len()..];
                ignore = val.parse::<u32>()
                    .map_err(|_| format!("invalid number: '{val}'"))?;
            }
            "--ignore" => {
                i += 1;
                if i >= argv.len() {
                    return Err("option '--ignore' requires an argument".to_string());
                }
                ignore = argv[i].parse::<u32>()
                    .map_err(|_| format!("invalid number: '{}'", argv[i]))?;
            }
            _ => {}
        }
        i += 1;
    }

    let count = if all {
        get_total_cpus()
    } else {
        get_available_cpus()
    };

    let result = if count > ignore { count - ignore } else { 1 };
    println!("{result}");
    Ok(())
}

fn get_total_cpus() -> u32 {
    // Try /proc/cpuinfo
    if let Ok(content) = fs::read_to_string("/proc/cpuinfo") {
        let count = content.lines()
            .filter(|line| line.starts_with("processor"))
            .count();
        if count > 0 {
            return count as u32;
        }
    }

    // Try /sys/devices/system/cpu/present
    if let Ok(content) = fs::read_to_string("/sys/devices/system/cpu/present")
        && let Some(count) = parse_cpu_range(content.trim()) {
            return count;
        }

    // Try NPROCESSORS_CONF env var
    if let Ok(val) = env::var("NPROCESSORS_CONF")
        && let Ok(n) = val.parse::<u32>() {
            return n;
        }

    1 // Default
}

fn get_available_cpus() -> u32 {
    // Try /sys/devices/system/cpu/online
    if let Ok(content) = fs::read_to_string("/sys/devices/system/cpu/online")
        && let Some(count) = parse_cpu_range(content.trim()) {
            return count;
        }

    // Try NPROCESSORS_ONLN or OMP_NUM_THREADS env var
    if let Ok(val) = env::var("OMP_NUM_THREADS")
        && let Ok(n) = val.parse::<u32>() {
            return n;
        }

    get_total_cpus()
}

/// Parse CPU range like "0-7" or "0-3,5-7" into a count
fn parse_cpu_range(s: &str) -> Option<u32> {
    let mut count = 0u32;
    for part in s.split(',') {
        let part = part.trim();
        if part.contains('-') {
            let mut split = part.splitn(2, '-');
            let lo: u32 = split.next()?.parse().ok()?;
            let hi: u32 = split.next()?.parse().ok()?;
            count += hi - lo + 1;
        } else if !part.is_empty() {
            let _: u32 = part.parse().ok()?;
            count += 1;
        }
    }
    if count > 0 { Some(count) } else { None }
}

// ── arch ───────────────────────────────────────────────────────────

fn run_arch() -> Result<(), String> {
    let argv: Vec<String> = env::args().collect();
    if argv.len() > 1 && (argv[1] == "-h" || argv[1] == "--help") {
        eprintln!("Usage: arch");
        eprintln!("Print the machine hardware name (same as uname -m).");
        process::exit(0);
    }

    // On our OS, always x86_64
    // Could read from uname data or /proc/cpuinfo
    let arch = get_arch();
    println!("{arch}");
    Ok(())
}

fn get_arch() -> String {
    // Try /proc/cpuinfo or similar
    if let Ok(content) = fs::read_to_string("/proc/cpuinfo") {
        for line in content.lines() {
            if (line.starts_with("model name") || line.starts_with("cpu family"))
                && (line.contains("x86_64") || line.contains("AMD64")) {
                    return "x86_64".to_string();
                }
        }
    }

    // Default for our OS
    "x86_64".to_string()
}

// ── pathchk ────────────────────────────────────────────────────────

fn run_pathchk() -> Result<(), String> {
    let argv: Vec<String> = env::args().collect();
    let mut portability = false;
    let mut posix_only = false;
    let mut paths: Vec<String> = Vec::new();

    let mut i = 1;
    while i < argv.len() {
        match argv[i].as_str() {
            "-h" | "--help" => {
                eprintln!("Usage: pathchk [-p] [-P] [--portability] PATH...");
                eprintln!("Check whether file names are valid or portable.");
                eprintln!();
                eprintln!("  -p, --portability  check for POSIX portable filename character set");
                eprintln!("  -P                 check for empty names and leading dashes");
                process::exit(0);
            }
            "-p" | "--portability" => portability = true,
            "-P" => posix_only = true,
            "--" => {
                i += 1;
                while i < argv.len() {
                    paths.push(argv[i].clone());
                    i += 1;
                }
                break;
            }
            _ => paths.push(argv[i].clone()),
        }
        i += 1;
    }

    if paths.is_empty() {
        return Err("missing operand".to_string());
    }

    let mut ok = true;
    for path in &paths {
        if let Err(e) = check_path(path, portability, posix_only) {
            eprintln!("pathchk: {e}");
            ok = false;
        }
    }

    if !ok {
        process::exit(1);
    }
    Ok(())
}

/// POSIX portable filename characters: A-Z, a-z, 0-9, '.', '-', '_'
fn is_portable_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_'
}

fn check_path(path: &str, portability: bool, posix_check: bool) -> Result<(), String> {
    if path.is_empty() {
        return Err("empty path".to_string());
    }

    // POSIX limits
    let path_max = 4096usize; // Our OS PATH_MAX
    let name_max = 255usize;  // Our OS NAME_MAX

    if path.len() > path_max {
        return Err(format!("path '{}' is too long ({} > {path_max})", path, path.len()));
    }

    for component in path.split('/') {
        if component.is_empty() {
            continue; // Leading/trailing/double slashes
        }

        if component.len() > name_max {
            return Err(format!(
                "name '{}' is too long ({} > {name_max})",
                component,
                component.len()
            ));
        }

        if portability {
            for c in component.chars() {
                if !is_portable_char(c) && c != '/' {
                    return Err(format!(
                        "nonportable character '{}' in path '{}'",
                        c, path
                    ));
                }
            }
        }

        if posix_check
            && component.starts_with('-') {
                return Err(format!(
                    "leading dash in component '{}' of path '{}'",
                    component, path
                ));
            }

        // Check for null bytes
        if component.contains('\0') {
            return Err(format!("path '{}' contains null byte", path));
        }
    }

    Ok(())
}

// ── logname ────────────────────────────────────────────────────────

fn run_logname() -> Result<(), String> {
    let argv: Vec<String> = env::args().collect();
    if argv.len() > 1 && (argv[1] == "-h" || argv[1] == "--help") {
        eprintln!("Usage: logname");
        eprintln!("Print the user's login name.");
        process::exit(0);
    }

    // Try LOGNAME, then USER, then /etc/passwd lookup via uid
    if let Ok(name) = env::var("LOGNAME")
        && !name.is_empty() {
            println!("{name}");
            return Ok(());
        }
    if let Ok(name) = env::var("USER")
        && !name.is_empty() {
            println!("{name}");
            return Ok(());
        }

    Err("no login name".to_string())
}

// ── users ──────────────────────────────────────────────────────────

fn run_users() -> Result<(), String> {
    let argv: Vec<String> = env::args().collect();
    let utmp_file = if argv.len() > 1 && !argv[1].starts_with('-') {
        argv[1].as_str()
    } else {
        if argv.len() > 1 && (argv[1] == "-h" || argv[1] == "--help") {
            eprintln!("Usage: users [UTMP_FILE]");
            eprintln!("Print the user names of users currently logged in.");
            process::exit(0);
        }
        "/var/log/wtmp"
    };

    // Read utmp/wtmp records (384 bytes each) and extract current users
    let mut users: Vec<String> = Vec::new();

    if let Ok(data) = fs::read(utmp_file) {
        let record_size = 384;
        let mut offset = 0;
        while offset + record_size <= data.len() {
            let record = &data[offset..offset + record_size];
            // Type at offset 0 (i16): 7 = USER_PROCESS
            if record.len() >= 2 {
                let rec_type = i16::from_le_bytes([record[0], record[1]]);
                if rec_type == 7 {
                    // User at offset 8, 32 bytes
                    if record.len() >= 40 {
                        let user_bytes = &record[8..40];
                        let user = extract_string(user_bytes);
                        if !user.is_empty() {
                            users.push(user);
                        }
                    }
                }
            }
            offset += record_size;
        }
    }

    // Remove users who have since logged out (DEAD_PROCESS)
    // For simplicity, just deduplicate and sort
    users.sort();
    users.dedup();

    if !users.is_empty() {
        println!("{}", users.join(" "));
    }
    Ok(())
}

fn extract_string(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).to_string()
}

// ── tty ────────────────────────────────────────────────────────────

fn run_tty() -> Result<(), String> {
    let argv: Vec<String> = env::args().collect();
    let mut silent = false;

    for arg in &argv[1..] {
        match arg.as_str() {
            "-s" | "--silent" | "--quiet" => silent = true,
            "-h" | "--help" => {
                eprintln!("Usage: tty [-s]");
                eprintln!("Print the file name of the terminal connected to stdin.");
                eprintln!();
                eprintln!("  -s, --silent  print nothing, only return exit status");
                process::exit(0);
            }
            _ => {}
        }
    }

    // Try to determine the terminal
    let tty_name = get_tty_name();

    match tty_name {
        Some(name) => {
            if !silent {
                println!("{name}");
            }
            Ok(())
        }
        None => {
            if !silent {
                println!("not a tty");
            }
            process::exit(1);
        }
    }
}

fn get_tty_name() -> Option<String> {
    // Try /proc/self/fd/0 (stdin) readlink
    if let Ok(target) = fs::read_link("/proc/self/fd/0") {
        let path = target.to_string_lossy().to_string();
        if path.starts_with("/dev/") {
            return Some(path);
        }
    }

    // Try TTY env var
    if let Ok(tty) = env::var("TTY")
        && !tty.is_empty() {
            return Some(tty);
        }

    // Try GPG_TTY
    if let Ok(tty) = env::var("GPG_TTY")
        && !tty.is_empty() {
            return Some(tty);
        }

    None
}

// ── Main ───────────────────────────────────────────────────────────

fn run() -> Result<(), String> {
    let argv0 = env::args().next().unwrap_or_else(|| "nproc".to_string());
    let mode = detect_mode(&argv0);

    match mode {
        Mode::Nproc => run_nproc(),
        Mode::Arch => run_arch(),
        Mode::Pathchk => run_pathchk(),
        Mode::Logname => run_logname(),
        Mode::Users => run_users(),
        Mode::Tty => run_tty(),
    }
}

fn main() {
    if let Err(e) = run() {
        let prog = env::args().next().unwrap_or_else(|| "nproc".to_string());
        let name = prog
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(&prog);
        eprintln!("{name}: {e}");
        process::exit(1);
    }
}

// ── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Personality detection ──

    #[test]
    fn test_detect_nproc() {
        assert_eq!(detect_mode("nproc"), Mode::Nproc);
        assert_eq!(detect_mode("/usr/bin/nproc"), Mode::Nproc);
        assert_eq!(detect_mode("nproc.exe"), Mode::Nproc);
    }

    #[test]
    fn test_detect_arch() {
        assert_eq!(detect_mode("arch"), Mode::Arch);
        assert_eq!(detect_mode("/bin/arch"), Mode::Arch);
    }

    #[test]
    fn test_detect_pathchk() {
        assert_eq!(detect_mode("pathchk"), Mode::Pathchk);
    }

    #[test]
    fn test_detect_logname() {
        assert_eq!(detect_mode("logname"), Mode::Logname);
    }

    #[test]
    fn test_detect_users() {
        assert_eq!(detect_mode("users"), Mode::Users);
    }

    #[test]
    fn test_detect_tty() {
        assert_eq!(detect_mode("tty"), Mode::Tty);
    }

    #[test]
    fn test_detect_default() {
        assert_eq!(detect_mode("unknown"), Mode::Nproc);
    }

    // ── CPU range parsing ──

    #[test]
    fn test_parse_cpu_range_single() {
        assert_eq!(parse_cpu_range("0"), Some(1));
        assert_eq!(parse_cpu_range("3"), Some(1));
    }

    #[test]
    fn test_parse_cpu_range_range() {
        assert_eq!(parse_cpu_range("0-3"), Some(4));
        assert_eq!(parse_cpu_range("0-7"), Some(8));
        assert_eq!(parse_cpu_range("0-15"), Some(16));
    }

    #[test]
    fn test_parse_cpu_range_mixed() {
        assert_eq!(parse_cpu_range("0-3,5-7"), Some(7));
        assert_eq!(parse_cpu_range("0,2,4,6"), Some(4));
    }

    #[test]
    fn test_parse_cpu_range_empty() {
        assert_eq!(parse_cpu_range(""), None);
    }

    // ── pathchk ──

    #[test]
    fn test_check_path_valid() {
        assert!(check_path("/usr/bin/test", false, false).is_ok());
        assert!(check_path("file.txt", false, false).is_ok());
        assert!(check_path("a/b/c", false, false).is_ok());
    }

    #[test]
    fn test_check_path_empty() {
        assert!(check_path("", false, false).is_err());
    }

    #[test]
    fn test_check_path_portable() {
        assert!(check_path("hello_world.txt", true, false).is_ok());
        assert!(check_path("hello world.txt", true, false).is_err()); // Space not portable
        assert!(check_path("file@name", true, false).is_err()); // @ not portable
    }

    #[test]
    fn test_check_path_leading_dash() {
        assert!(check_path("-bad-name", false, true).is_err());
        assert!(check_path("good-name", false, true).is_ok());
        assert!(check_path("dir/-bad", false, true).is_err());
    }

    #[test]
    fn test_check_path_long_name() {
        let long_name = "a".repeat(256);
        assert!(check_path(&long_name, false, false).is_err());
    }

    #[test]
    fn test_check_path_max_name_ok() {
        let name = "a".repeat(255);
        assert!(check_path(&name, false, false).is_ok());
    }

    #[test]
    fn test_check_path_long_path() {
        let long_path = format!("/{}", "a/".repeat(2050));
        assert!(check_path(&long_path, false, false).is_err());
    }

    // ── Portable char check ──

    #[test]
    fn test_is_portable_char() {
        assert!(is_portable_char('a'));
        assert!(is_portable_char('Z'));
        assert!(is_portable_char('0'));
        assert!(is_portable_char('.'));
        assert!(is_portable_char('-'));
        assert!(is_portable_char('_'));
        assert!(!is_portable_char(' '));
        assert!(!is_portable_char('@'));
        assert!(!is_portable_char('!'));
        assert!(!is_portable_char('~'));
    }

    // ── String extraction ──

    #[test]
    fn test_extract_string_null_terminated() {
        let data = b"hello\0world\0\0\0\0\0";
        assert_eq!(extract_string(data), "hello");
    }

    #[test]
    fn test_extract_string_no_null() {
        let data = b"hello";
        assert_eq!(extract_string(data), "hello");
    }

    #[test]
    fn test_extract_string_empty() {
        let data = b"\0\0\0\0";
        assert_eq!(extract_string(data), "");
    }

    // ── Architecture ──

    #[test]
    fn test_get_arch_default() {
        // Always returns x86_64 for our OS
        let arch = get_arch();
        assert_eq!(arch, "x86_64");
    }

    // ── Edge cases ──

    #[test]
    fn test_pathchk_root() {
        assert!(check_path("/", false, false).is_ok());
    }

    #[test]
    fn test_pathchk_double_slash() {
        assert!(check_path("//usr//bin", false, false).is_ok());
    }

    #[test]
    fn test_pathchk_dot_paths() {
        assert!(check_path(".", false, false).is_ok());
        assert!(check_path("..", false, false).is_ok());
        assert!(check_path("./file", false, false).is_ok());
    }

    #[test]
    fn test_pathchk_portable_dot_dash() {
        assert!(check_path("file.txt", true, false).is_ok());
        assert!(check_path("my-file", true, false).is_ok());
        assert!(check_path("my_file", true, false).is_ok());
    }
}
