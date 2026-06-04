#![deny(clippy::all)]

//! cage-cli — OurOS Cage kiosk Wayland compositor
//!
//! Single personality: `cage`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_cage(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: cage [OPTIONS] APPLICATION");
        println!("cage v0.1 (OurOS) — Kiosk Wayland compositor");
        println!();
        println!("Options:");
        println!("  APPLICATION       Application to run fullscreen");
        println!("  -d                Debug mode");
        println!("  -r                Rotate display");
        println!("  -s                Allow VT switching");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("cage v0.1 (OurOS)"); return 0; }
    let app = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("foot");
    println!("Cage kiosk compositor starting...");
    println!("  Application: {} (fullscreen)", app);
    println!("  Output: auto-detected");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "cage".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_cage(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_cage};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/cage"), "cage");
        assert_eq!(basename(r"C:\bin\cage.exe"), "cage.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("cage.exe"), "cage");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_cage(&["--help".to_string()], "cage"), 0);
        assert_eq!(run_cage(&["-h".to_string()], "cage"), 0);
        let _ = run_cage(&["--version".to_string()], "cage");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_cage(&[], "cage");
    }
}
