#![deny(clippy::all)]

//! rofi-wayland-cli — OurOS rofi-wayland application launcher
//!
//! Single personality: `rofi`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_rofi(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: rofi [OPTIONS]");
        println!("rofi v1.7 (OurOS, Wayland fork) — Application launcher / window switcher");
        println!();
        println!("Options:");
        println!("  -show MODE        Show mode: drun, run, window, ssh, combi");
        println!("  -modi MODES       Enabled modes (comma-separated)");
        println!("  -theme THEME      Theme file");
        println!("  -dmenu            dmenu compatibility mode");
        println!("  -p PROMPT         Prompt text");
        println!("  -i                Case-insensitive");
        println!("  -lines N          Number of lines");
        println!("  -width N          Width (characters or %)");
        println!("  -location N       Location (0=center, 1-8=edges/corners)");
        println!("  -matching MODE    Matching: normal, regex, glob, fuzzy");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("rofi v1.7.5+wayland2 (OurOS)"); return 0; }
    if args.iter().any(|a| a == "-dmenu") {
        println!("rofi: dmenu mode (reading stdin)");
        return 0;
    }
    let mode = args.iter().skip_while(|a| a.as_str() != "-show").nth(1)
        .map(|s| s.as_str()).unwrap_or("drun");
    println!("rofi: {} mode", mode);
    println!("  [Search...                    ]");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "rofi".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_rofi(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_rofi};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/rofi-wayland"), "rofi-wayland");
        assert_eq!(basename(r"C:\bin\rofi-wayland.exe"), "rofi-wayland.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("rofi-wayland.exe"), "rofi-wayland");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_rofi(&["--help".to_string()], "rofi-wayland"), 0);
        assert_eq!(run_rofi(&["-h".to_string()], "rofi-wayland"), 0);
        assert_eq!(run_rofi(&["--version".to_string()], "rofi-wayland"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_rofi(&[], "rofi-wayland"), 0);
    }
}
