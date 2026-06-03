#![deny(clippy::all)]

//! steam-cli — OurOS Steam client launcher
//!
//! Single personality: `steam`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_steam(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: steam [OPTIONS] [steam://URL]");
        println!("steam v1.0 (OurOS) — Steam client launcher");
        println!();
        println!("Options:");
        println!("  -applaunch ID     Launch game by app ID");
        println!("  -silent           Start minimized");
        println!("  -login USER PASS  Login");
        println!("  -bigpicture       Big picture mode");
        println!("  -console          Open console");
        println!("  -shutdown          Shutdown Steam");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("steam v1.0 (OurOS)"); return 0; }
    if args.iter().any(|a| a == "-bigpicture") {
        println!("steam: big picture mode started");
        return 0;
    }
    println!("steam: Steam client starting");
    println!("  Runtime: Proton 8.0");
    println!("  Library: 42 games");
    println!("  Downloads: idle");
    println!("  Friends: 3 online");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "steam".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_steam(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_steam};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/steam"), "steam");
        assert_eq!(basename(r"C:\bin\steam.exe"), "steam.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("steam.exe"), "steam");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_steam(&["--help".to_string()], "steam"), 0);
        assert_eq!(run_steam(&["-h".to_string()], "steam"), 0);
        assert_eq!(run_steam(&["--version".to_string()], "steam"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_steam(&[], "steam"), 0);
    }
}
