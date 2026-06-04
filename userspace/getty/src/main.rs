// OurOS getty — virtual terminal login manager
//
// Multi-personality binary:
//   getty / agetty  — open a terminal, set its mode, prompt for login name, invoke login(1)
//   mingetty        — minimal getty for virtual consoles (no serial support)
//
// This is the userspace process that manages virtual terminal login sessions.
// It opens a tty, optionally configures baud rate and terminal settings,
// displays /etc/issue, prints a login prompt, reads the username, and exec's
// login(1) with that username.
//
// Usage:
//   getty [OPTIONS] <port> [baud_rate...]
//   agetty [OPTIONS] <port> [baud_rate...]
//   mingetty [OPTIONS] <tty>

#![cfg_attr(not(test), no_main)]
// Config::term_type and Termios::{echo, canonical, cr_to_nl} encode the
// TERM environment variable and the c_lflag/c_iflag bits the real getty
// pumps into tcsetattr(2). The stub only exercises the line discipline
// surface needed to print /etc/issue and read a username; the rest is
// preserved for the future driver-attached implementation.
#![allow(dead_code)]

#[cfg(not(test))]
use std::env;
use std::io::{self, BufRead, Read, Write};
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Personality detection
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Personality {
    Getty,    // full agetty
    Mingetty, // minimal, virtual-console only
}

fn detect_personality(argv0: &str) -> Personality {
    let base = argv0.rsplit('/').next().unwrap_or(argv0);
    let base = base.rsplit('\\').next().unwrap_or(base);
    let lower = base.to_ascii_lowercase();
    let lower = lower.strip_suffix(".exe").unwrap_or(&lower);
    match lower {
        "mingetty" => Personality::Mingetty,
        _ => Personality::Getty, // getty, agetty all map to full
    }
}

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct Config {
    personality: Personality,
    port: String,
    baud_rates: Vec<u32>,
    term_type: String,
    autologin_user: Option<String>,
    no_issue: bool,
    issue_file: PathBuf,
    login_program: PathBuf,
    no_hostname: bool,
    no_newline: bool,
    long_hostname: bool,
    local_line: bool,
    no_reset: bool,
    no_clear: bool,
    skip_login: bool,
    login_pause: bool,
    chroot_dir: Option<PathBuf>,
    init_string: Option<String>,
    nice_value: Option<i32>,
    delay: Option<u32>,
    timeout: Option<u32>,
    erase_char: Option<char>,
    kill_char: Option<char>,
    host: Option<String>,
    keep_baud: bool,
    show_help: bool,
    show_version: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            personality: Personality::Getty,
            port: String::new(),
            baud_rates: vec![9600],
            term_type: String::from("linux"),
            autologin_user: None,
            no_issue: false,
            issue_file: PathBuf::from("/etc/issue"),
            login_program: PathBuf::from("/bin/login"),
            no_hostname: false,
            no_newline: false,
            long_hostname: false,
            local_line: false,
            no_reset: false,
            no_clear: false,
            skip_login: false,
            login_pause: false,
            chroot_dir: None,
            init_string: None,
            nice_value: None,
            delay: None,
            timeout: None,
            erase_char: None,
            kill_char: None,
            host: None,
            keep_baud: false,
            show_help: false,
            show_version: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Argument parsing
// ---------------------------------------------------------------------------

fn parse_args(args: &[String]) -> Result<Config, String> {
    let personality = args
        .first()
        .map(|a| detect_personality(a))
        .unwrap_or(Personality::Getty);

    let mut cfg = Config {
        personality,
        ..Default::default()
    };

    let mut i = 1;
    let mut positional = Vec::new();

    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            "-h" | "--help" => cfg.show_help = true,
            "-V" | "--version" => cfg.show_version = true,
            "-8" | "--8bits" => {} // accept but no-op in our implementation
            "-a" | "--autologin" => {
                i += 1;
                cfg.autologin_user = Some(
                    args.get(i)
                        .ok_or("-a requires a username")?
                        .clone(),
                );
            }
            "-c" | "--noreset" => cfg.no_reset = true,
            "-E" | "--remote" => {} // accept, no-op
            "-f" | "--issue-file" => {
                i += 1;
                cfg.issue_file = PathBuf::from(
                    args.get(i).ok_or("-f requires a filename")?,
                );
            }
            "-H" | "--host" => {
                i += 1;
                cfg.host = Some(
                    args.get(i)
                        .ok_or("-H requires a hostname")?
                        .clone(),
                );
            }
            "-i" | "--noissue" => cfg.no_issue = true,
            "-I" | "--init-string" => {
                i += 1;
                cfg.init_string = Some(
                    args.get(i)
                        .ok_or("-I requires a string")?
                        .clone(),
                );
            }
            "-J" | "--noclear" => cfg.no_clear = true,
            "-l" | "--login-program" => {
                i += 1;
                cfg.login_program = PathBuf::from(
                    args.get(i).ok_or("-l requires a program path")?,
                );
            }
            "-L" | "--local-line" => cfg.local_line = true,
            "-m" | "--extract-baud" => cfg.keep_baud = true,
            "-n" | "--skip-login" => cfg.skip_login = true,
            "-N" | "--nonewline" => cfg.no_newline = true,
            "-o" | "--long-hostname" => cfg.long_hostname = true,
            "-p" | "--login-pause" => cfg.login_pause = true,
            "-r" | "--chroot" => {
                i += 1;
                cfg.chroot_dir = Some(PathBuf::from(
                    args.get(i).ok_or("-r requires a directory")?,
                ));
            }
            "-R" | "--hangup" => {} // accept, no-op
            "-s" | "--keep-baud" => cfg.keep_baud = true,
            "-t" | "--timeout" => {
                i += 1;
                cfg.timeout = Some(
                    args.get(i)
                        .ok_or("-t requires a number")?
                        .parse::<u32>()
                        .map_err(|e| format!("-t: {e}"))?,
                );
            }
            "-U" | "--detect-case" => {} // accept, no-op
            "-w" | "--wait-cr" => {} // accept, no-op
            "--nohints" => {} // accept, no-op
            "--nohostname" => cfg.no_hostname = true,
            "--erase-chars" => {
                i += 1;
                let s = args.get(i).ok_or("--erase-chars requires a char")?;
                cfg.erase_char = s.chars().next();
            }
            "--kill-chars" => {
                i += 1;
                let s = args.get(i).ok_or("--kill-chars requires a char")?;
                cfg.kill_char = s.chars().next();
            }
            "--delay" => {
                i += 1;
                cfg.delay = Some(
                    args.get(i)
                        .ok_or("--delay requires a number")?
                        .parse::<u32>()
                        .map_err(|e| format!("--delay: {e}"))?,
                );
            }
            "--nice" => {
                i += 1;
                cfg.nice_value = Some(
                    args.get(i)
                        .ok_or("--nice requires a number")?
                        .parse::<i32>()
                        .map_err(|e| format!("--nice: {e}"))?,
                );
            }
            other if other.starts_with('-') => {
                return Err(format!("unknown option: {other}"));
            }
            _ => positional.push(arg.clone()),
        }
        i += 1;
    }

    // Parse positional: port [baud_rate...]
    match personality {
        Personality::Mingetty => {
            if let Some(port) = positional.first() {
                cfg.port = port.clone();
            }
            // mingetty doesn't use baud rates
        }
        Personality::Getty => {
            if let Some(port) = positional.first() {
                cfg.port = port.clone();
            }
            if positional.len() > 1 {
                cfg.baud_rates.clear();
                for baud_str in &positional[1..] {
                    // baud rates can be comma-separated
                    for piece in baud_str.split(',') {
                        let b = piece
                            .trim()
                            .parse::<u32>()
                            .map_err(|e| format!("invalid baud rate '{piece}': {e}"))?;
                        cfg.baud_rates.push(b);
                    }
                }
            }
        }
    }

    Ok(cfg)
}

// ---------------------------------------------------------------------------
// Issue file processing
// ---------------------------------------------------------------------------

/// Process /etc/issue escape sequences
fn process_issue_line(line: &str, hostname: &str, tty_name: &str, os_name: &str, os_release: &str) -> String {
    let mut result = String::with_capacity(line.len());
    let mut chars = line.chars();

    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('s') => result.push_str(os_name),
                Some('n') => result.push_str(hostname),
                Some('r') => result.push_str(os_release),
                Some('v') => result.push_str("#1"),
                Some('m') => result.push_str("x86_64"),
                Some('l') => result.push_str(tty_name),
                Some('o') => result.push_str("(none)"),
                Some('O') => result.push_str("(none)"),
                Some('d') => result.push_str(&get_date()),
                Some('t') => result.push_str(&get_time()),
                Some('u') | Some('U') => result.push_str("1 user"),
                Some('\\') => result.push('\\'),
                Some(other) => {
                    result.push('\\');
                    result.push(other);
                }
                None => result.push('\\'),
            }
        } else {
            result.push(c);
        }
    }

    result
}

fn get_date() -> String {
    // Simplified date - would read from system clock in real OS
    String::from("1970-01-01")
}

fn get_time() -> String {
    String::from("00:00:00")
}

/// Read and display issue file
fn display_issue(
    writer: &mut dyn Write,
    issue_path: &Path,
    hostname: &str,
    tty_name: &str,
) -> io::Result<()> {
    let os_name = "OurOS";
    let os_release = "0.1.0";

    let content = match std::fs::read_to_string(issue_path) {
        Ok(c) => c,
        Err(_) => return Ok(()), // no issue file is not an error
    };

    for line in content.lines() {
        let processed = process_issue_line(line, hostname, tty_name, os_name, os_release);
        writeln!(writer, "{processed}")?;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Terminal setup
// ---------------------------------------------------------------------------

/// Terminal line settings (simplified representation)
#[derive(Debug, Clone)]
struct TermSettings {
    baud_rate: u32,
    erase_char: char,
    kill_char: char,
    echo: bool,
    canonical: bool,
    cr_to_nl: bool,
}

impl Default for TermSettings {
    fn default() -> Self {
        Self {
            baud_rate: 9600,
            erase_char: '\x7f', // DEL
            kill_char: '\x15',  // Ctrl-U
            echo: true,
            canonical: true,
            cr_to_nl: true,
        }
    }
}

fn setup_terminal(cfg: &Config) -> TermSettings {
    let mut term = TermSettings::default();

    if let Some(baud) = cfg.baud_rates.first() {
        term.baud_rate = *baud;
    }

    if let Some(ec) = cfg.erase_char {
        term.erase_char = ec;
    }

    if let Some(kc) = cfg.kill_char {
        term.kill_char = kc;
    }

    term
}

// ---------------------------------------------------------------------------
// Login name reading
// ---------------------------------------------------------------------------

/// Read a login name from the terminal
fn read_login_name(reader: &mut dyn BufRead, writer: &mut dyn Write) -> io::Result<Option<String>> {
    let mut buf = String::new();
    let n = reader.read_line(&mut buf)?;
    if n == 0 {
        return Ok(None); // EOF
    }

    let name = buf.trim().to_string();
    if name.is_empty() {
        return Ok(None);
    }

    // Validate: login names should be alphanumeric, underscore, hyphen, dot
    for ch in name.chars() {
        if !ch.is_alphanumeric() && ch != '_' && ch != '-' && ch != '.' {
            writeln!(writer, "Invalid character in login name: '{ch}'")?;
            return Ok(None);
        }
    }

    // Length check
    if name.len() > 256 {
        writeln!(writer, "Login name too long")?;
        return Ok(None);
    }

    Ok(Some(name))
}

// ---------------------------------------------------------------------------
// Hostname resolution
// ---------------------------------------------------------------------------

fn get_hostname(long: bool) -> String {
    // Try /etc/hostname first
    if let Ok(name) = std::fs::read_to_string("/etc/hostname") {
        let name = name.trim().to_string();
        if !name.is_empty() {
            if long {
                return name;
            }
            // Short hostname: first component
            return name.split('.').next().unwrap_or(&name).to_string();
        }
    }
    String::from("localhost")
}

// ---------------------------------------------------------------------------
// TTY path helpers
// ---------------------------------------------------------------------------

fn tty_path(port: &str) -> PathBuf {
    if port.starts_with('/') {
        PathBuf::from(port)
    } else {
        PathBuf::from(format!("/dev/{port}"))
    }
}

fn tty_short_name(port: &str) -> &str {
    if let Some(stripped) = port.strip_prefix("/dev/") {
        stripped
    } else {
        port
    }
}

// ---------------------------------------------------------------------------
// Help and version
// ---------------------------------------------------------------------------

fn print_help(personality: Personality) {
    match personality {
        Personality::Getty => {
            println!("Usage: getty [OPTIONS] <port> [baud_rate[,baud_rate]...]");
            println!("       agetty [OPTIONS] <port> [baud_rate[,baud_rate]...]");
            println!();
            println!("Open a terminal line, set its mode, and invoke the login program.");
            println!();
            println!("Options:");
            println!("  -a, --autologin <user>    Auto-login the specified user");
            println!("  -c, --noreset             Don't reset terminal cflags");
            println!("  -f, --issue-file <file>   Display specified issue file");
            println!("  -H, --host <host>         Specify login host");
            println!("  -i, --noissue             Don't display /etc/issue");
            println!("  -I, --init-string <str>   Send init string before anything else");
            println!("  -J, --noclear             Don't clear the screen");
            println!("  -l, --login-program <prog> Use specified login program");
            println!("  -L, --local-line          Force local line (no modem control)");
            println!("  -m, --extract-baud        Extract baud rate from modem status");
            println!("  -n, --skip-login          Don't prompt for login name");
            println!("  -N, --nonewline           Don't print newline before issue");
            println!("  -o, --long-hostname        Show full qualified hostname");
            println!("  -p, --login-pause          Wait for keypress before login prompt");
            println!("  -r, --chroot <dir>        Chroot before login");
            println!("  -s, --keep-baud           Keep existing baud rate");
            println!("  -t, --timeout <secs>      Timeout for login name input");
            println!("  --nohostname              Don't show hostname in prompt");
            println!("  --erase-chars <char>      Additional erase character");
            println!("  --kill-chars <char>       Additional kill character");
            println!("  --delay <msecs>           Delay before opening tty");
            println!("  --nice <value>            Run with adjusted nice value");
            println!("  -h, --help                Show this help");
            println!("  -V, --version             Show version");
        }
        Personality::Mingetty => {
            println!("Usage: mingetty [OPTIONS] <tty>");
            println!();
            println!("Minimal getty for virtual consoles.");
            println!();
            println!("Options:");
            println!("  -a, --autologin <user>    Auto-login the specified user");
            println!("  -i, --noissue             Don't display /etc/issue");
            println!("  -l, --login-program <prog> Use specified login program");
            println!("  --noclear                 Don't clear the screen");
            println!("  --long-hostname           Show full qualified hostname");
            println!("  -h, --help                Show this help");
            println!("  -V, --version             Show version");
        }
    }
}

fn print_version(personality: Personality) {
    let name = match personality {
        Personality::Getty => "getty (agetty)",
        Personality::Mingetty => "mingetty",
    };
    println!("{name} (OurOS) 0.1.0");
}

// ---------------------------------------------------------------------------
// VT100 control sequences
// ---------------------------------------------------------------------------

fn vt_clear_screen(writer: &mut dyn Write) -> io::Result<()> {
    writer.write_all(b"\x1b[H\x1b[2J")
}

fn vt_reset(writer: &mut dyn Write) -> io::Result<()> {
    writer.write_all(b"\x1bc")
}

// ---------------------------------------------------------------------------
// Main getty loop
// ---------------------------------------------------------------------------

fn run_getty(
    cfg: &Config,
    reader: &mut dyn BufRead,
    writer: &mut dyn Write,
) -> Result<Option<(PathBuf, Vec<String>)>, String> {
    let hostname = get_hostname(cfg.long_hostname);
    let tty_name = tty_short_name(&cfg.port);

    // Reset terminal if requested
    if !cfg.no_reset {
        vt_reset(writer).map_err(|e| format!("reset terminal: {e}"))?;
    }

    // Clear screen if requested
    if !cfg.no_clear {
        vt_clear_screen(writer).map_err(|e| format!("clear screen: {e}"))?;
    }

    // Setup terminal settings
    let _term = setup_terminal(cfg);

    // Send init string if specified
    if let Some(ref init) = cfg.init_string {
        writer
            .write_all(init.as_bytes())
            .map_err(|e| format!("init string: {e}"))?;
    }

    // Autologin mode
    if let Some(ref user) = cfg.autologin_user {
        let mut login_args = vec![
            cfg.login_program.display().to_string(),
            String::from("-f"),
            user.clone(),
        ];
        if let Some(ref host) = cfg.host {
            login_args.push(String::from("-h"));
            login_args.push(host.clone());
        }
        return Ok(Some((cfg.login_program.clone(), login_args)));
    }

    // Display issue file
    if !cfg.no_issue {
        if !cfg.no_newline {
            writeln!(writer).map_err(|e| format!("write: {e}"))?;
        }
        display_issue(writer, &cfg.issue_file, &hostname, tty_name)
            .map_err(|e| format!("display issue: {e}"))?;
    }

    // Login pause
    if cfg.login_pause {
        write!(writer, "Press any key to continue...")
            .map_err(|e| format!("write: {e}"))?;
        writer.flush().map_err(|e| format!("flush: {e}"))?;
        let mut one = [0u8; 1];
        let _ = std::io::stdin().read(&mut one);
        writeln!(writer).map_err(|e| format!("write: {e}"))?;
    }

    // Show login prompt and read username
    loop {
        // Build prompt
        if !cfg.no_hostname {
            write!(writer, "{hostname} ").map_err(|e| format!("write: {e}"))?;
        }
        write!(writer, "login: ").map_err(|e| format!("write: {e}"))?;
        writer.flush().map_err(|e| format!("flush: {e}"))?;

        // Skip login mode - just exec login without username
        if cfg.skip_login {
            let mut login_args = vec![cfg.login_program.display().to_string()];
            if let Some(ref host) = cfg.host {
                login_args.push(String::from("-h"));
                login_args.push(host.clone());
            }
            return Ok(Some((cfg.login_program.clone(), login_args)));
        }

        // Read login name
        match read_login_name(reader, writer) {
            Ok(Some(username)) => {
                let mut login_args = vec![
                    cfg.login_program.display().to_string(),
                    String::from("--"),
                    username,
                ];
                if let Some(ref host) = cfg.host {
                    login_args.push(String::from("-h"));
                    login_args.push(host.clone());
                }
                return Ok(Some((cfg.login_program.clone(), login_args)));
            }
            Ok(None) => {
                // Empty input or EOF, loop again (or exit on EOF)
                writeln!(writer).map_err(|e| format!("write: {e}"))?;
                continue;
            }
            Err(e) => {
                return Err(format!("read login name: {e}"));
            }
        }
    }
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
            eprintln!("getty: {e}");
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

    match run_getty(&cfg, &mut reader, &mut writer) {
        Ok(Some((_program, args))) => {
            // In a real OS, we would exec() the login program here.
            // For now, print what we would execute.
            eprintln!("getty: would exec: {}", args.join(" "));
            0
        }
        Ok(None) => 0,
        Err(e) => {
            eprintln!("getty: {e}");
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
    fn test_detect_personality_getty() {
        assert_eq!(detect_personality("getty"), Personality::Getty);
        assert_eq!(detect_personality("agetty"), Personality::Getty);
        assert_eq!(detect_personality("/sbin/getty"), Personality::Getty);
        assert_eq!(detect_personality("/sbin/agetty"), Personality::Getty);
    }

    #[test]
    fn test_detect_personality_mingetty() {
        assert_eq!(detect_personality("mingetty"), Personality::Mingetty);
        assert_eq!(detect_personality("/sbin/mingetty"), Personality::Mingetty);
    }

    #[test]
    fn test_parse_args_basic() {
        let args = vec![
            "getty".to_string(),
            "tty1".to_string(),
        ];
        let cfg = parse_args(&args).unwrap();
        assert_eq!(cfg.port, "tty1");
        assert_eq!(cfg.personality, Personality::Getty);
    }

    #[test]
    fn test_parse_args_with_baud() {
        let args = vec![
            "getty".to_string(),
            "ttyS0".to_string(),
            "115200,9600".to_string(),
        ];
        let cfg = parse_args(&args).unwrap();
        assert_eq!(cfg.port, "ttyS0");
        assert_eq!(cfg.baud_rates, vec![115200, 9600]);
    }

    #[test]
    fn test_parse_args_autologin() {
        let args = vec![
            "getty".to_string(),
            "-a".to_string(),
            "root".to_string(),
            "tty1".to_string(),
        ];
        let cfg = parse_args(&args).unwrap();
        assert_eq!(cfg.autologin_user, Some("root".to_string()));
        assert_eq!(cfg.port, "tty1");
    }

    #[test]
    fn test_parse_args_noissue() {
        let args = vec![
            "getty".to_string(),
            "-i".to_string(),
            "tty1".to_string(),
        ];
        let cfg = parse_args(&args).unwrap();
        assert!(cfg.no_issue);
    }

    #[test]
    fn test_parse_args_login_program() {
        let args = vec![
            "getty".to_string(),
            "-l".to_string(),
            "/usr/bin/login".to_string(),
            "tty1".to_string(),
        ];
        let cfg = parse_args(&args).unwrap();
        assert_eq!(cfg.login_program, PathBuf::from("/usr/bin/login"));
    }

    #[test]
    fn test_parse_args_timeout() {
        let args = vec![
            "getty".to_string(),
            "-t".to_string(),
            "60".to_string(),
            "tty1".to_string(),
        ];
        let cfg = parse_args(&args).unwrap();
        assert_eq!(cfg.timeout, Some(60));
    }

    #[test]
    fn test_parse_args_host() {
        let args = vec![
            "getty".to_string(),
            "-H".to_string(),
            "remote.host".to_string(),
            "tty1".to_string(),
        ];
        let cfg = parse_args(&args).unwrap();
        assert_eq!(cfg.host, Some("remote.host".to_string()));
    }

    #[test]
    fn test_parse_args_skip_login() {
        let args = vec![
            "getty".to_string(),
            "-n".to_string(),
            "tty1".to_string(),
        ];
        let cfg = parse_args(&args).unwrap();
        assert!(cfg.skip_login);
    }

    #[test]
    fn test_parse_args_noclear() {
        let args = vec![
            "getty".to_string(),
            "-J".to_string(),
            "tty1".to_string(),
        ];
        let cfg = parse_args(&args).unwrap();
        assert!(cfg.no_clear);
    }

    #[test]
    fn test_parse_args_noreset() {
        let args = vec![
            "getty".to_string(),
            "-c".to_string(),
            "tty1".to_string(),
        ];
        let cfg = parse_args(&args).unwrap();
        assert!(cfg.no_reset);
    }

    #[test]
    fn test_parse_args_chroot() {
        let args = vec![
            "getty".to_string(),
            "-r".to_string(),
            "/mnt/root".to_string(),
            "tty1".to_string(),
        ];
        let cfg = parse_args(&args).unwrap();
        assert_eq!(cfg.chroot_dir, Some(PathBuf::from("/mnt/root")));
    }

    #[test]
    fn test_parse_args_multiple_baud_separate() {
        let args = vec![
            "getty".to_string(),
            "ttyS0".to_string(),
            "115200".to_string(),
            "57600".to_string(),
            "9600".to_string(),
        ];
        let cfg = parse_args(&args).unwrap();
        assert_eq!(cfg.baud_rates, vec![115200, 57600, 9600]);
    }

    #[test]
    fn test_parse_args_unknown_option() {
        let args = vec![
            "getty".to_string(),
            "--badopt".to_string(),
        ];
        assert!(parse_args(&args).is_err());
    }

    #[test]
    fn test_parse_args_help() {
        let args = vec![
            "getty".to_string(),
            "--help".to_string(),
        ];
        let cfg = parse_args(&args).unwrap();
        assert!(cfg.show_help);
    }

    #[test]
    fn test_parse_args_version() {
        let args = vec![
            "getty".to_string(),
            "-V".to_string(),
        ];
        let cfg = parse_args(&args).unwrap();
        assert!(cfg.show_version);
    }

    #[test]
    fn test_parse_args_mingetty() {
        let args = vec![
            "mingetty".to_string(),
            "tty1".to_string(),
        ];
        let cfg = parse_args(&args).unwrap();
        assert_eq!(cfg.personality, Personality::Mingetty);
        assert_eq!(cfg.port, "tty1");
    }

    #[test]
    fn test_parse_args_init_string() {
        let args = vec![
            "getty".to_string(),
            "-I".to_string(),
            "ATZ\r".to_string(),
            "ttyS0".to_string(),
        ];
        let cfg = parse_args(&args).unwrap();
        assert_eq!(cfg.init_string, Some("ATZ\r".to_string()));
    }

    #[test]
    fn test_parse_args_erase_kill_chars() {
        let args = vec![
            "getty".to_string(),
            "--erase-chars".to_string(),
            "#".to_string(),
            "--kill-chars".to_string(),
            "@".to_string(),
            "tty1".to_string(),
        ];
        let cfg = parse_args(&args).unwrap();
        assert_eq!(cfg.erase_char, Some('#'));
        assert_eq!(cfg.kill_char, Some('@'));
    }

    #[test]
    fn test_process_issue_line_hostname() {
        let result = process_issue_line("Welcome to \\n", "myhost", "tty1", "OurOS", "0.1.0");
        assert_eq!(result, "Welcome to myhost");
    }

    #[test]
    fn test_process_issue_line_os() {
        let result = process_issue_line("\\s \\r", "myhost", "tty1", "OurOS", "0.1.0");
        assert_eq!(result, "OurOS 0.1.0");
    }

    #[test]
    fn test_process_issue_line_tty() {
        let result = process_issue_line("on \\l", "myhost", "tty1", "OurOS", "0.1.0");
        assert_eq!(result, "on tty1");
    }

    #[test]
    fn test_process_issue_line_arch() {
        let result = process_issue_line("\\m", "myhost", "tty1", "OurOS", "0.1.0");
        assert_eq!(result, "x86_64");
    }

    #[test]
    fn test_process_issue_line_escape() {
        let result = process_issue_line("\\\\path", "myhost", "tty1", "OurOS", "0.1.0");
        assert_eq!(result, "\\path");
    }

    #[test]
    fn test_process_issue_line_unknown_escape() {
        let result = process_issue_line("\\x", "myhost", "tty1", "OurOS", "0.1.0");
        assert_eq!(result, "\\x");
    }

    #[test]
    fn test_process_issue_line_no_escapes() {
        let result = process_issue_line("Hello World", "myhost", "tty1", "OurOS", "0.1.0");
        assert_eq!(result, "Hello World");
    }

    #[test]
    fn test_tty_path_absolute() {
        assert_eq!(tty_path("/dev/tty1"), PathBuf::from("/dev/tty1"));
    }

    #[test]
    fn test_tty_path_relative() {
        assert_eq!(tty_path("tty1"), PathBuf::from("/dev/tty1"));
    }

    #[test]
    fn test_tty_short_name() {
        assert_eq!(tty_short_name("/dev/tty1"), "tty1");
        assert_eq!(tty_short_name("tty1"), "tty1");
        assert_eq!(tty_short_name("/dev/ttyS0"), "ttyS0");
    }

    #[test]
    fn test_read_login_name_valid() {
        let input = b"testuser\n";
        let mut reader = Cursor::new(input.as_slice());
        let mut writer = Vec::new();
        let result = read_login_name(&mut reader, &mut writer).unwrap();
        assert_eq!(result, Some("testuser".to_string()));
    }

    #[test]
    fn test_read_login_name_empty() {
        let input = b"\n";
        let mut reader = Cursor::new(input.as_slice());
        let mut writer = Vec::new();
        let result = read_login_name(&mut reader, &mut writer).unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn test_read_login_name_eof() {
        let input = b"";
        let mut reader = Cursor::new(input.as_slice());
        let mut writer = Vec::new();
        let result = read_login_name(&mut reader, &mut writer).unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn test_read_login_name_with_dot() {
        let input = b"john.doe\n";
        let mut reader = Cursor::new(input.as_slice());
        let mut writer = Vec::new();
        let result = read_login_name(&mut reader, &mut writer).unwrap();
        assert_eq!(result, Some("john.doe".to_string()));
    }

    #[test]
    fn test_read_login_name_with_hyphen() {
        let input = b"test-user\n";
        let mut reader = Cursor::new(input.as_slice());
        let mut writer = Vec::new();
        let result = read_login_name(&mut reader, &mut writer).unwrap();
        assert_eq!(result, Some("test-user".to_string()));
    }

    #[test]
    fn test_read_login_name_with_underscore() {
        let input = b"test_user\n";
        let mut reader = Cursor::new(input.as_slice());
        let mut writer = Vec::new();
        let result = read_login_name(&mut reader, &mut writer).unwrap();
        assert_eq!(result, Some("test_user".to_string()));
    }

    #[test]
    fn test_read_login_name_invalid_chars() {
        let input = b"test user\n";
        let mut reader = Cursor::new(input.as_slice());
        let mut writer = Vec::new();
        let result = read_login_name(&mut reader, &mut writer).unwrap();
        assert_eq!(result, None); // space is invalid
    }

    #[test]
    fn test_setup_terminal_defaults() {
        let cfg = Config::default();
        let term = setup_terminal(&cfg);
        assert_eq!(term.baud_rate, 9600);
        assert_eq!(term.erase_char, '\x7f');
        assert_eq!(term.kill_char, '\x15');
    }

    #[test]
    fn test_setup_terminal_custom_baud() {
        let cfg = Config {
            baud_rates: vec![115200, 9600],
            ..Config::default()
        };
        let term = setup_terminal(&cfg);
        assert_eq!(term.baud_rate, 115200);
    }

    #[test]
    fn test_setup_terminal_custom_erase() {
        let cfg = Config {
            erase_char: Some('#'),
            ..Config::default()
        };
        let term = setup_terminal(&cfg);
        assert_eq!(term.erase_char, '#');
    }

    #[test]
    fn test_run_getty_autologin() {
        let cfg = Config {
            autologin_user: Some("root".to_string()),
            port: "tty1".to_string(),
            no_reset: true,
            no_clear: true,
            ..Default::default()
        };
        let input = b"";
        let mut reader = Cursor::new(input.as_slice());
        let mut writer = Vec::new();
        let result = run_getty(&cfg, &mut reader, &mut writer).unwrap();
        assert!(result.is_some());
        let (prog, args) = result.unwrap();
        assert_eq!(prog, PathBuf::from("/bin/login"));
        assert!(args.contains(&"-f".to_string()));
        assert!(args.contains(&"root".to_string()));
    }

    #[test]
    fn test_run_getty_skip_login() {
        let cfg = Config {
            skip_login: true,
            port: "tty1".to_string(),
            no_reset: true,
            no_clear: true,
            no_issue: true,
            ..Default::default()
        };
        let input = b"";
        let mut reader = Cursor::new(input.as_slice());
        let mut writer = Vec::new();
        let result = run_getty(&cfg, &mut reader, &mut writer).unwrap();
        assert!(result.is_some());
        let (prog, args) = result.unwrap();
        assert_eq!(prog, PathBuf::from("/bin/login"));
        assert!(!args.contains(&"--".to_string()));
    }

    #[test]
    fn test_run_getty_normal_login() {
        let cfg = Config {
            port: "tty1".to_string(),
            no_reset: true,
            no_clear: true,
            no_issue: true,
            ..Default::default()
        };
        let input = b"testuser\n";
        let mut reader = Cursor::new(input.as_slice());
        let mut writer = Vec::new();
        let result = run_getty(&cfg, &mut reader, &mut writer).unwrap();
        assert!(result.is_some());
        let (prog, args) = result.unwrap();
        assert_eq!(prog, PathBuf::from("/bin/login"));
        assert!(args.contains(&"testuser".to_string()));
    }

    #[test]
    fn test_run_getty_with_host() {
        let cfg = Config {
            autologin_user: Some("root".to_string()),
            host: Some("remote.host".to_string()),
            port: "tty1".to_string(),
            no_reset: true,
            no_clear: true,
            ..Default::default()
        };
        let input = b"";
        let mut reader = Cursor::new(input.as_slice());
        let mut writer = Vec::new();
        let result = run_getty(&cfg, &mut reader, &mut writer).unwrap();
        let (_prog, args) = result.unwrap();
        assert!(args.contains(&"-h".to_string()));
        assert!(args.contains(&"remote.host".to_string()));
    }

    #[test]
    fn test_display_issue_missing_file() {
        let mut writer = Vec::new();
        let result = display_issue(
            &mut writer,
            Path::new("/nonexistent/issue"),
            "myhost",
            "tty1",
        );
        assert!(result.is_ok());
        assert!(writer.is_empty()); // no output for missing file
    }

    #[test]
    fn test_default_config() {
        let cfg = Config::default();
        assert_eq!(cfg.personality, Personality::Getty);
        assert_eq!(cfg.baud_rates, vec![9600]);
        assert_eq!(cfg.term_type, "linux");
        assert!(!cfg.no_issue);
        assert_eq!(cfg.issue_file, PathBuf::from("/etc/issue"));
        assert_eq!(cfg.login_program, PathBuf::from("/bin/login"));
    }

    #[test]
    fn test_parse_args_nice() {
        let args = vec![
            "getty".to_string(),
            "--nice".to_string(),
            "10".to_string(),
            "tty1".to_string(),
        ];
        let cfg = parse_args(&args).unwrap();
        assert_eq!(cfg.nice_value, Some(10));
    }

    #[test]
    fn test_parse_args_delay() {
        let args = vec![
            "getty".to_string(),
            "--delay".to_string(),
            "500".to_string(),
            "tty1".to_string(),
        ];
        let cfg = parse_args(&args).unwrap();
        assert_eq!(cfg.delay, Some(500));
    }

    #[test]
    fn test_parse_args_keep_baud() {
        let args = vec![
            "getty".to_string(),
            "-s".to_string(),
            "ttyS0".to_string(),
        ];
        let cfg = parse_args(&args).unwrap();
        assert!(cfg.keep_baud);
    }

    #[test]
    fn test_parse_args_local_line() {
        let args = vec![
            "getty".to_string(),
            "-L".to_string(),
            "ttyS0".to_string(),
        ];
        let cfg = parse_args(&args).unwrap();
        assert!(cfg.local_line);
    }

    #[test]
    fn test_parse_args_nonewline() {
        let args = vec![
            "getty".to_string(),
            "-N".to_string(),
            "tty1".to_string(),
        ];
        let cfg = parse_args(&args).unwrap();
        assert!(cfg.no_newline);
    }

    #[test]
    fn test_parse_args_login_pause() {
        let args = vec![
            "getty".to_string(),
            "-p".to_string(),
            "tty1".to_string(),
        ];
        let cfg = parse_args(&args).unwrap();
        assert!(cfg.login_pause);
    }

    #[test]
    fn test_parse_args_long_hostname() {
        let args = vec![
            "getty".to_string(),
            "-o".to_string(),
            "tty1".to_string(),
        ];
        let cfg = parse_args(&args).unwrap();
        assert!(cfg.long_hostname);
    }

    #[test]
    fn test_parse_args_nohostname() {
        let args = vec![
            "getty".to_string(),
            "--nohostname".to_string(),
            "tty1".to_string(),
        ];
        let cfg = parse_args(&args).unwrap();
        assert!(cfg.no_hostname);
    }

    #[test]
    fn test_vt_sequences() {
        let mut buf = Vec::new();
        vt_clear_screen(&mut buf).unwrap();
        assert_eq!(buf, b"\x1b[H\x1b[2J");

        buf.clear();
        vt_reset(&mut buf).unwrap();
        assert_eq!(buf, b"\x1bc");
    }
}
