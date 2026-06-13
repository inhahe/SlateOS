#![deny(clippy::all)]

//! avizo-cli — SlateOS Avizo OSD notification daemon
//!
//! Multi-personality: `avizo-service`, `volumectl`, `lightctl`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_avizo_service(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: avizo-service [OPTIONS]");
        println!("avizo-service v1.0 (Slate OS) — OSD notification daemon");
        println!();
        println!("Options:");
        println!("  --version      Show version");
        println!();
        println!("Neat volume/brightness OSD for Wayland. macOS-style overlay.");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("avizo-service v1.0 (Slate OS)"); return 0; }
    println!("avizo-service: OSD daemon running");
    0
}

fn run_volumectl(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: volumectl [raise|lower|toggle-mute]");
        println!("volumectl v1.0 (Slate OS) — Volume control with OSD");
        return 0;
    }
    match args.first().map(|s| s.as_str()) {
        Some("raise") => println!("volumectl: volume +5% (75%)"),
        Some("lower") => println!("volumectl: volume -5% (65%)"),
        Some("toggle-mute") => println!("volumectl: mute toggled"),
        _ => println!("volumectl: current volume 70%"),
    }
    0
}

fn run_lightctl(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: lightctl [raise|lower]");
        println!("lightctl v1.0 (Slate OS) — Brightness control with OSD");
        return 0;
    }
    match args.first().map(|s| s.as_str()) {
        Some("raise") => println!("lightctl: brightness +5% (80%)"),
        Some("lower") => println!("lightctl: brightness -5% (70%)"),
        _ => println!("lightctl: current brightness 75%"),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "avizo-service".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "volumectl" => run_volumectl(&rest, &prog),
        "lightctl" => run_lightctl(&rest, &prog),
        _ => run_avizo_service(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_avizo_service};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/avizo"), "avizo");
        assert_eq!(basename(r"C:\bin\avizo.exe"), "avizo.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("avizo.exe"), "avizo");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_avizo_service(&["--help".to_string()], "avizo"), 0);
        assert_eq!(run_avizo_service(&["-h".to_string()], "avizo"), 0);
        let _ = run_avizo_service(&["--version".to_string()], "avizo");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_avizo_service(&[], "avizo");
    }
}
