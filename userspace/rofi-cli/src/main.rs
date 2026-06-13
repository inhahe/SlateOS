#![deny(clippy::all)]

//! rofi-cli — SlateOS Rofi application launcher
//!
//! Multi-personality: `rofi`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_rofi(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: rofi [OPTIONS]");
        println!("rofi 1.7.5 (Slate OS) — Window switcher, app launcher, dmenu replacement");
        println!();
        println!("Options:");
        println!("  -show MODE     Show mode (drun, run, window, ssh, combi, keys)");
        println!("  -modi MODES    Enabled modes (comma-separated)");
        println!("  -theme THEME   Theme name or path");
        println!("  -dmenu         dmenu-compatible mode");
        println!("  -p PROMPT      Prompt text");
        println!("  -mesg TEXT     Message text");
        println!("  -filter TEXT   Initial filter");
        println!("  -i             Case insensitive");
        println!("  -lines N       Number of lines");
        println!("  -width N       Width (pixels or %)");
        println!("  -location N    Window location (0-8)");
        println!("  -dump-config   Dump current config");
        println!("  -dump-theme    Dump current theme");
        println!("  --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-version") {
        println!("rofi: Version: 1.7.5");
        return 0;
    }
    let dmenu = args.iter().any(|a| a == "-dmenu");
    let show = args.windows(2).find(|w| w[0] == "-show")
        .map(|w| w[1].as_str());

    if dmenu {
        println!("(dmenu mode: reading from stdin)");
    } else if let Some(mode) = show {
        match mode {
            "drun" => println!("(showing application launcher)"),
            "run" => println!("(showing command runner)"),
            "window" => println!("(showing window switcher)"),
            "ssh" => println!("(showing SSH connections)"),
            _ => println!("(showing {} mode)", mode),
        }
    } else if args.iter().any(|a| a == "-dump-config") {
        println!("configuration {{");
        println!("  modi: \"drun,run,window,ssh\";");
        println!("  font: \"Mono 12\";");
        println!("  show-icons: true;");
        println!("  terminal: \"alacritty\";");
        println!("  location: 0;");
        println!("  yoffset: 0;");
        println!("  xoffset: 0;");
        println!("}}");
    } else {
        println!("rofi: no mode specified. Use -show <mode>");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "rofi".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_rofi(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_rofi};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/rofi"), "rofi");
        assert_eq!(basename(r"C:\bin\rofi.exe"), "rofi.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("rofi.exe"), "rofi");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_rofi(&["--help".to_string()]), 0);
        assert_eq!(run_rofi(&["-h".to_string()]), 0);
        let _ = run_rofi(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_rofi(&[]);
    }
}
