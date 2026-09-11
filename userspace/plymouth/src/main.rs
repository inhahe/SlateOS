//! Slate OS boot splash system.
//!
//! Multi-personality binary providing:
//! - **plymouth** — boot splash client
//! - **plymouthd** — boot splash daemon
//! - **plymouth-set-default-theme** — theme management
//!
//! Provides a graphical boot splash that hides text console output
//! during boot, showing a progress indicator and/or animation.

#![deny(clippy::all)]

use quoting::quoteaf_os;
use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process;

const VERSION: &str = "0.1.0";

// ============================================================================
// Configuration
// ============================================================================

const PLYMOUTH_CONF: &str = "/etc/plymouth/plymouthd.conf";
const THEMES_DIR: &str = "/usr/share/plymouth/themes";
const PID_FILE: &str = "/run/plymouth/pid";

#[derive(Clone, Debug)]
struct PlymouthConfig {
    theme: String,
    show_delay: u32,
    device_timeout: u32,
    _device_scale: u32,
}

impl Default for PlymouthConfig {
    fn default() -> Self {
        Self {
            theme: "spinner".to_string(),
            show_delay: 0,
            device_timeout: 8,
            _device_scale: 1,
        }
    }
}

fn load_config() -> PlymouthConfig {
    let mut config = PlymouthConfig::default();
    if let Ok(content) = fs::read_to_string(PLYMOUTH_CONF) {
        let mut in_daemon = false;
        for line in content.lines() {
            let line = line.trim();
            if line == "[Daemon]" {
                in_daemon = true;
                continue;
            }
            if line.starts_with('[') {
                in_daemon = false;
                continue;
            }
            if !in_daemon {
                continue;
            }
            if let Some((key, value)) = line.split_once('=') {
                let key = key.trim();
                let value = value.trim();
                match key {
                    "Theme" => config.theme = value.to_string(),
                    "ShowDelay" => {
                        config.show_delay = value.parse().unwrap_or(0);
                    }
                    "DeviceTimeout" => {
                        config.device_timeout = value.parse().unwrap_or(8);
                    }
                    _ => {}
                }
            }
        }
    }
    config
}

fn save_config(config: &PlymouthConfig) -> io::Result<()> {
    if let Some(parent) = Path::new(PLYMOUTH_CONF).parent() {
        fs::create_dir_all(parent)?;
    }
    let content = format!(
        "[Daemon]\nTheme={}\nShowDelay={}\nDeviceTimeout={}\n",
        config.theme, config.show_delay, config.device_timeout
    );
    fs::write(PLYMOUTH_CONF, content)
}

// ============================================================================
// Theme management
// ============================================================================

#[derive(Clone, Debug)]
struct ThemeInfo {
    name: String,
    description: String,
    _module_name: String,
    _path: PathBuf,
}

fn list_themes() -> Vec<ThemeInfo> {
    let mut themes = Vec::new();
    if let Ok(entries) = fs::read_dir(THEMES_DIR) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let name = entry.file_name().to_string_lossy().to_string();
                let plymouth_file = path.join(format!("{name}.plymouth"));
                let info = if let Ok(content) = fs::read_to_string(&plymouth_file) {
                    parse_theme_file(&name, &content, &path)
                } else {
                    ThemeInfo {
                        name: name.clone(),
                        description: String::new(),
                        _module_name: name,
                        _path: path,
                    }
                };
                themes.push(info);
            }
        }
    }

    // Add built-in themes if directory doesn't exist.
    if themes.is_empty() {
        let builtins = [
            ("spinner", "A simple spinner", "two-step"),
            ("fade-in", "Fade in and out", "fade-throbber"),
            ("solar", "Solar flare animation", "space-flares"),
            ("bgrt", "UEFI firmware logo", "bgrt"),
            ("text", "Text-mode progress", "text"),
            ("details", "Detailed text boot messages", "details"),
            ("tribar", "Tri-color bar", "tribar"),
        ];
        for (name, desc, module) in &builtins {
            themes.push(ThemeInfo {
                name: name.to_string(),
                description: desc.to_string(),
                _module_name: module.to_string(),
                _path: PathBuf::from(THEMES_DIR).join(name),
            });
        }
    }

    themes.sort_by(|a, b| a.name.cmp(&b.name));
    themes
}

fn parse_theme_file(name: &str, content: &str, path: &Path) -> ThemeInfo {
    let mut description = String::new();
    let mut _module_name = name.to_string();

    for line in content.lines() {
        let line = line.trim();
        if let Some((key, value)) = line.split_once('=') {
            let key = key.trim();
            let value = value.trim();
            match key {
                "Description" => description = value.to_string(),
                "ModuleName" => _module_name = value.to_string(),
                _ => {}
            }
        }
    }

    ThemeInfo {
        name: name.to_string(),
        description,
        _module_name,
        _path: path.to_path_buf(),
    }
}

fn get_default_theme() -> String {
    let config = load_config();
    config.theme
}

fn set_default_theme(theme: &str) -> io::Result<()> {
    // Verify theme exists.
    let themes = list_themes();
    if !themes.iter().any(|t| t.name == theme) {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("theme {} not found", quoteaf_os(theme)),
        ));
    }

    let mut config = load_config();
    config.theme = theme.to_string();
    save_config(&config)
}

// ============================================================================
// Daemon state
// ============================================================================

#[derive(Clone, Debug)]
struct _DaemonState {
    mode: BootMode,
    _progress: f32,
    _message: String,
    _password_prompt: Option<String>,
    _splash_shown: bool,
    _theme: String,
    _pid: u32,
}

#[derive(Clone, Debug, PartialEq)]
enum BootMode {
    Boot,
    Shutdown,
    Reboot,
    Updates,
}

impl Default for _DaemonState {
    fn default() -> Self {
        Self {
            mode: BootMode::Boot,
            _progress: 0.0,
            _message: String::new(),
            _password_prompt: None,
            _splash_shown: false,
            _theme: "spinner".to_string(),
            _pid: 0,
        }
    }
}

// ============================================================================
// plymouth client personality
// ============================================================================

fn plymouth_main(args: &[String]) -> i32 {
    if args.is_empty() {
        println!("Usage: plymouth <command> [options]");
        println!();
        println!("Commands:");
        println!("  --ping              Check if daemon is running");
        println!("  --quit              Quit the daemon");
        println!("  --hide-splash       Hide the splash");
        println!("  --show-splash       Show the splash");
        println!("  --update=TEXT       Update status text");
        println!("  --message=TEXT      Display message");
        println!("  --ask-for-password  Prompt for password");
        println!("  --ask-question=Q    Ask a yes/no question");
        println!("  --display-message=T Display a message");
        println!("  --hide-message=T    Hide a message");
        println!("  --wait              Wait for daemon to quit");
        return 0;
    }

    // plymouth dispatches on a single command word; only the first arg is used.
    if let Some(arg) = args.first() {
        match arg.as_str() {
            "--ping" => {
                if is_daemon_running() {
                    println!("plymouth: daemon is running");
                    return 0;
                } else {
                    println!("plymouth: daemon is not running");
                    return 1;
                }
            }
            "--quit" => {
                eprintln!("plymouth: cannot send quit to a daemon: none is running");
                return 1;
            }
            "--show-splash" => {
                eprintln!("plymouth: cannot show the splash on a daemon: none is running");
                return 1;
            }
            "--hide-splash" => {
                eprintln!("plymouth: cannot hide the splash on a daemon: none is running");
                return 1;
            }
            "--wait" => {
                eprintln!("plymouth: cannot wait for a daemon: none is running");
                return 1;
            }
            s if s.starts_with("--update=") => {
                let text = s.strip_prefix("--update=").unwrap_or("");
                eprintln!("plymouth: would update status: {text}");
                return 0;
            }
            s if s.starts_with("--message=") || s.starts_with("--display-message=") => {
                let text = s.split_once('=').map(|(_, v)| v).unwrap_or("");
                eprintln!("plymouth: would display message: {text}");
                return 0;
            }
            s if s.starts_with("--hide-message=") => {
                let text = s.strip_prefix("--hide-message=").unwrap_or("");
                eprintln!("plymouth: would hide message: {text}");
                return 0;
            }
            "--ask-for-password" => {
                eprint!("Password: ");
                let _ = io::stderr().flush();
                let mut pw = String::new();
                let _ = io::stdin().read_line(&mut pw);
                print!("{}", pw.trim());
                return 0;
            }
            s if s.starts_with("--ask-question=") => {
                let question = s.strip_prefix("--ask-question=").unwrap_or("");
                eprint!("{question} ");
                let _ = io::stderr().flush();
                let mut answer = String::new();
                let _ = io::stdin().read_line(&mut answer);
                print!("{}", answer.trim());
                return 0;
            }
            "--help" | "-h" => {
                println!("Usage: plymouth <command> [options]");
                println!();
                println!("Plymouth boot splash client.");
                println!();
                println!("  --ping, --quit, --show-splash, --hide-splash");
                println!("  --update=TEXT, --message=TEXT, --wait");
                println!("  --ask-for-password, --ask-question=Q");
                println!("  --version, --help");
                return 0;
            }
            "--version" => {
                println!("plymouth (Slate OS) {VERSION}");
                return 0;
            }
            other => {
                eprintln!("plymouth: unknown command {}", quoteaf_os(other));
                return 1;
            }
        }
    }

    0
}

/// Is a plymouth daemon actually running?
///
/// This was `Path::new(PID_FILE).exists()`, which is the classic stale-pidfile
/// defect: a daemon that crashed, was killed, or exited without unlinking its
/// file leaves `plymouth --ping` answering "daemon is running" for as long as
/// the file survives -- which on `/run` is until the next boot. The file says
/// a daemon *once started*, and the question asked is whether one is *there
/// now*.
///
/// It reads the pid and asks the kernel. `kill(pid, 0)` sends no signal; it
/// performs the permission-and-existence check and nothing else, which is
/// exactly this question. `posix/src/signal.rs` exports it and routes it to
/// `SYS_SIGNAL_SEND`.
fn is_daemon_running() -> bool {
    daemon_running_from(Path::new(PID_FILE))
}

/// The body of [`is_daemon_running`], taking the path so it can be tested.
///
/// The stale-pidfile case is the whole point of this function, and it cannot
/// be exercised against a hard-coded `/run/plymouth/pid`.
fn daemon_running_from(pid_file: &Path) -> bool {
    let Ok(text) = fs::read_to_string(pid_file) else {
        return false;
    };
    let Ok(pid) = text.trim().parse::<i32>() else {
        // A pid file we cannot parse is not evidence of a running daemon. It
        // is evidence of a corrupt file, and answering "yes" to that would be
        // the same mistake in a smaller place.
        return false;
    };
    if pid <= 0 {
        return false;
    }
    process_exists(pid)
}

/// Does process `pid` exist and are we permitted to signal it?
#[cfg(unix)]
fn process_exists(pid: i32) -> bool {
    // SAFETY: `kill` takes two integers by value and returns one. Signal 0
    // delivers nothing -- it is defined as the existence-and-permission probe.
    unsafe extern "C" {
        fn kill(pid: i32, sig: i32) -> i32;
    }
    // SAFETY: as above.
    unsafe { kill(pid, 0) == 0 }
}

/// The development host has no `kill(2)`. Reporting "not running" is the
/// answer that cannot invent a daemon; the alternative would have `--ping`
/// claim one on a machine that has never run this program.
#[cfg(not(unix))]
fn process_exists(_pid: i32) -> bool {
    false
}

// ============================================================================
// plymouthd daemon personality
// ============================================================================

fn plymouthd_main(args: &[String]) -> i32 {
    let mut mode = BootMode::Boot;
    let mut no_daemon = false;
    let mut attach_to_session = false;
    let mut _tty: Option<String> = None;
    let mut _kernel_cmdline: Option<String> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--mode=boot" => mode = BootMode::Boot,
            "--mode=shutdown" => mode = BootMode::Shutdown,
            "--mode=reboot" => mode = BootMode::Reboot,
            "--mode=updates" => mode = BootMode::Updates,
            "--no-daemon" => no_daemon = true,
            "--attach-to-session" => attach_to_session = true,
            "--tty" => {
                i += 1;
                if i < args.len() {
                    _tty = Some(args[i].clone());
                }
            }
            "--kernel-command-line" => {
                i += 1;
                if i < args.len() {
                    _kernel_cmdline = Some(args[i].clone());
                }
            }
            "--help" | "-h" => {
                println!("Usage: plymouthd [options]");
                println!();
                println!("Options:");
                println!("  --mode=MODE             boot, shutdown, reboot, updates");
                println!("  --no-daemon             Don't daemonize");
                println!("  --attach-to-session     Attach to existing session");
                println!("  --tty TTY               TTY to use");
                println!("  --kernel-command-line L  Override kernel cmdline");
                println!("  --help                  Display this help");
                println!("  --version               Display version");
                return 0;
            }
            "--version" => {
                println!("plymouthd (Slate OS) {VERSION}");
                return 0;
            }
            s if s.starts_with("--mode=") => {
                let m = s.strip_prefix("--mode=").unwrap_or("boot");
                mode = match m {
                    "shutdown" => BootMode::Shutdown,
                    "reboot" => BootMode::Reboot,
                    "updates" => BootMode::Updates,
                    _ => BootMode::Boot,
                };
            }
            other => {
                eprintln!("plymouthd: unknown option {}", quoteaf_os(other));
                return 1;
            }
        }
        i += 1;
    }

    let config = load_config();

    eprintln!(
        "plymouthd: starting with theme {}, mode {mode:?}",
        quoteaf_os(&config.theme)
    );
    eprintln!("plymouthd: no_daemon={no_daemon}, attach={attach_to_session}");

    // NO PID FILE, AND NO RUN DIRECTORY.
    //
    // This used to `create_dir_all(RUN_DIR)`, write its own pid to
    // `/run/plymouth/pid`, announce "daemon would enter main loop (simulated,
    // exiting)", and unlink the file on the way out. The unlink made it mostly
    // harmless -- but only mostly: `let _ =` discarded its failure, and a
    // plymouthd killed between the write and the unlink leaves a pid file that
    // `--ping` used to read as a running daemon.
    //
    // That is `bootctl`'s shape, fixed in the same session: a command that
    // cannot do its job should not leave behind the artefact by which its job
    // is later recognised. Writing a pid file is a claim that a process is
    // there to receive signals, and there is not one.
    eprintln!("plymouthd: cannot start: this build has no splash renderer, so a");
    eprintln!("plymouthd: daemon would have nothing to draw on and no client");
    eprintln!("plymouthd: could reach it. Exiting rather than idling.");
    1
}

// ============================================================================
// plymouth-set-default-theme personality
// ============================================================================

fn set_theme_main(args: &[String]) -> i32 {
    let mut rebuild = false;
    let mut list = false;
    let mut theme_name: Option<String> = None;

    for arg in args {
        match arg.as_str() {
            "-R" | "--rebuild-initrd" => rebuild = true,
            "-l" | "--list" => list = true,
            "--help" | "-h" => {
                println!("Usage: plymouth-set-default-theme [options] [theme]");
                println!();
                println!("Set or query the default Plymouth theme.");
                println!();
                println!("Options:");
                println!("  -R, --rebuild-initrd  Rebuild initramfs after setting theme");
                println!("  -l, --list            List available themes");
                println!("  -h, --help            Display this help");
                println!("  --version             Display version");
                println!();
                println!("Without arguments, print the current default theme.");
                return 0;
            }
            "--version" => {
                println!("plymouth-set-default-theme (Slate OS) {VERSION}");
                return 0;
            }
            s if !s.starts_with('-') => {
                theme_name = Some(s.to_string());
            }
            other => {
                eprintln!(
                    "plymouth-set-default-theme: unknown option {}",
                    quoteaf_os(other)
                );
                return 1;
            }
        }
    }

    if list {
        let themes = list_themes();
        for theme in &themes {
            if theme.description.is_empty() {
                println!("{}", theme.name);
            } else {
                println!("{} - {}", theme.name, theme.description);
            }
        }
        return 0;
    }

    match theme_name {
        Some(name) => match set_default_theme(&name) {
            Ok(()) => {
                println!(
                    "plymouth-set-default-theme: theme set to {}",
                    quoteaf_os(&name)
                );
                if rebuild {
                    eprintln!("plymouth-set-default-theme: would rebuild initramfs");
                }
                0
            }
            Err(e) => {
                eprintln!("plymouth-set-default-theme: {e}");
                1
            }
        },
        None => {
            println!("{}", get_default_theme());
            0
        }
    }
}

// ============================================================================
// Main dispatch
// ============================================================================

fn main() {
    let args: Vec<String> = env::args().collect();

    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("plymouth");
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' {
                last_sep = i + 1;
            }
        }
        let base = &s[last_sep..];
        let base = base.strip_suffix(".exe").unwrap_or(base);
        base.to_string()
    };

    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let exit_code = match prog_name.as_str() {
        "plymouthd" => plymouthd_main(&rest),
        "plymouth-set-default-theme" => set_theme_main(&rest),
        _ => plymouth_main(&rest),
    };

    process::exit(exit_code);
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// A pid file naming a process that is not there is not a running daemon.
    ///
    /// `is_daemon_running` was `Path::new(PID_FILE).exists()`, so any pid file
    /// left by a daemon that crashed or was killed made `plymouth --ping`
    /// answer "daemon is running" until the next boot cleared `/run`. The file
    /// records that a daemon once STARTED; the question is whether one is
    /// there NOW.
    ///
    /// 0x7FFF_FFFE is used rather than a small number because a low pid is
    /// likely to be a real process on any host this runs on, which would make
    /// the test pass for the wrong reason.
    #[test]
    fn a_pid_file_naming_a_dead_process_is_not_a_running_daemon() {
        let scratch = scratchdir::ScratchDir::new("plymouth-stale");
        let pid_file = scratch.path("pid");
        fs::write(&pid_file, "2147483646\n").expect("write pid file");
        assert!(
            !daemon_running_from(&pid_file),
            "a pid file alone must not count as a running daemon"
        );
    }

    #[test]
    fn a_missing_pid_file_is_not_a_running_daemon() {
        let scratch = scratchdir::ScratchDir::new("plymouth-missing");
        assert!(!daemon_running_from(&scratch.path("pid")));
    }

    /// Garbage is evidence of a corrupt file, not of a daemon.
    #[test]
    fn an_unparsable_pid_file_is_not_a_running_daemon() {
        let scratch = scratchdir::ScratchDir::new("plymouth-garbage");
        for content in ["", "   ", "not a number", "12x", "-1", "0"] {
            let pid_file = scratch.path("pid");
            fs::write(&pid_file, content).expect("write");
            assert!(
                !daemon_running_from(&pid_file),
                "{content:?} should not read as a running daemon"
            );
        }
    }

    /// The other direction, so the function is not merely always-false: our
    /// own pid IS alive. Only on unix, where `kill(2)` exists -- the host arm
    /// answers false by construction and this would assert the opposite.
    #[cfg(unix)]
    #[test]
    fn a_pid_file_naming_a_live_process_is_a_running_daemon() {
        let scratch = scratchdir::ScratchDir::new("plymouth-live");
        let pid_file = scratch.path("pid");
        fs::write(&pid_file, format!("{}\n", std::process::id())).expect("write");
        assert!(
            daemon_running_from(&pid_file),
            "our own pid is alive, so this must be true -- otherwise the test \
             above passes for the wrong reason"
        );
    }

    #[test]
    fn test_default_config() {
        let config = PlymouthConfig::default();
        assert_eq!(config.theme, "spinner");
        assert_eq!(config.show_delay, 0);
        assert_eq!(config.device_timeout, 8);
    }

    #[test]
    fn test_list_themes() {
        let themes = list_themes();
        assert!(!themes.is_empty());
    }

    #[test]
    fn test_builtin_themes() {
        let themes = list_themes();
        let names: Vec<&str> = themes.iter().map(|t| t.name.as_str()).collect();
        // At minimum, built-ins should include these.
        assert!(names.contains(&"spinner") || names.contains(&"text"));
    }

    #[test]
    fn test_get_default_theme() {
        let theme = get_default_theme();
        assert!(!theme.is_empty());
    }

    #[test]
    fn test_daemon_state_default() {
        let state = _DaemonState::default();
        assert_eq!(state.mode, BootMode::Boot);
        assert_eq!(state._progress, 0.0);
        assert!(state._message.is_empty());
    }

    #[test]
    fn test_boot_modes() {
        assert_ne!(BootMode::Boot, BootMode::Shutdown);
        assert_ne!(BootMode::Reboot, BootMode::Updates);
        assert_eq!(BootMode::Boot, BootMode::Boot);
    }

    #[test]
    fn test_is_daemon_not_running() {
        assert!(!is_daemon_running());
    }

    #[test]
    fn test_parse_theme_file() {
        let content = "Name=My Theme\nDescription=A fancy theme\nModuleName=fancy-renderer\n";
        let info = parse_theme_file("mytheme", content, Path::new("/themes/mytheme"));
        assert_eq!(info.name, "mytheme");
        assert_eq!(info.description, "A fancy theme");
        assert_eq!(info._module_name, "fancy-renderer");
    }

    #[test]
    fn test_parse_theme_file_empty() {
        let info = parse_theme_file("empty", "", Path::new("/themes/empty"));
        assert_eq!(info.name, "empty");
        assert!(info.description.is_empty());
        assert_eq!(info._module_name, "empty"); // Defaults to name.
    }

    #[test]
    fn test_theme_info_path() {
        let info = ThemeInfo {
            name: "test".to_string(),
            description: "Test theme".to_string(),
            _module_name: "test".to_string(),
            _path: PathBuf::from("/usr/share/plymouth/themes/test"),
        };
        assert_eq!(info._path.file_name().unwrap().to_str().unwrap(), "test");
    }

    #[test]
    fn test_set_theme_nonexistent() {
        // Should fail for a theme that doesn't exist in the standard location.
        let result = set_default_theme("nonexistent_theme_xyz_123");
        assert!(result.is_err());
    }
}
