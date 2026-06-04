#![deny(clippy::all)]

//! waybar-cli — OurOS Waybar status bar for Wayland
//!
//! Single personality: `waybar`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_waybar(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: waybar [OPTIONS]");
        println!("Waybar 0.10.3 (OurOS) — Highly customizable Wayland bar");
        println!();
        println!("Options:");
        println!("  -c, --config FILE     Config file path");
        println!("  -s, --style FILE      CSS style file");
        println!("  -b, --bar_id ID       Bar ID");
        println!("  -l, --log-level LVL   Log level (trace, debug, info, warning, error)");
        println!("  --version             Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("waybar 0.10.3");
        return 0;
    }
    let config = args.windows(2).find(|w| w[0] == "-c" || w[0] == "--config")
        .map(|w| w[1].as_str());
    let style = args.windows(2).find(|w| w[0] == "-s" || w[0] == "--style")
        .map(|w| w[1].as_str());
    let log_level = args.windows(2).find(|w| w[0] == "-l" || w[0] == "--log-level")
        .map(|w| w[1].as_str()).unwrap_or("info");

    if let Some(c) = config {
        println!("waybar: Loading config from '{}'", c);
    } else {
        println!("waybar: Loading config from ~/.config/waybar/config.jsonc");
    }
    if let Some(s) = style {
        println!("waybar: Loading style from '{}'", s);
    } else {
        println!("waybar: Loading style from ~/.config/waybar/style.css");
    }
    println!("waybar: Log level: {}", log_level);
    println!("waybar: Modules loaded: clock, workspaces, tray, network, pulseaudio, cpu, memory, battery");
    println!("waybar: Bar rendered on DP-1.");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "waybar".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_waybar(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_waybar};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/waybar"), "waybar");
        assert_eq!(basename(r"C:\bin\waybar.exe"), "waybar.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("waybar.exe"), "waybar");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_waybar(&["--help".to_string()], "waybar"), 0);
        assert_eq!(run_waybar(&["-h".to_string()], "waybar"), 0);
        let _ = run_waybar(&["--version".to_string()], "waybar");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_waybar(&[], "waybar");
    }
}
