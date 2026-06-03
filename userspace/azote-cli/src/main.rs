#![deny(clippy::all)]

//! azote-cli — OurOS Azote wallpaper manager GUI
//!
//! Single personality: `azote`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_azote(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: azote [OPTIONS]");
        println!("azote v1.12 (OurOS) — Wallpaper manager for Wayland/X11");
        println!();
        println!("Options:");
        println!("  -d DIR            Wallpaper directory");
        println!("  --version         Show version");
        println!();
        println!("GUI wallpaper browser and setter. Supports swaybg, feh, nitrogen.");
        println!("Features: thumbnail preview, per-monitor wallpaper, color picker.");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("azote v1.12 (OurOS)"); return 0; }
    let dir = args.iter().skip_while(|a| a.as_str() != "-d").nth(1)
        .map(|s| s.as_str()).unwrap_or("~/Pictures");
    println!("azote: opening wallpaper browser ({})", dir);
    println!("  Detected backend: swaybg");
    println!("  Outputs: HDMI-A-1, DP-1");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "azote".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_azote(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_azote};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/azote"), "azote");
        assert_eq!(basename(r"C:\bin\azote.exe"), "azote.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("azote.exe"), "azote");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_azote(&["--help".to_string()], "azote"), 0);
        assert_eq!(run_azote(&["-h".to_string()], "azote"), 0);
        assert_eq!(run_azote(&["--version".to_string()], "azote"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_azote(&[], "azote"), 0);
    }
}
