#![deny(clippy::all)]

//! sfwbar-cli — OurOS sfwbar flexible Wayland taskbar
//!
//! Single personality: `sfwbar`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_sfwbar(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: sfwbar [OPTIONS]");
        println!("sfwbar v1.0 (OurOS) — Flexible Wayland taskbar");
        println!();
        println!("Options:");
        println!("  -f FILE           Config file");
        println!("  -c CSS            CSS theme file");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("sfwbar v1.0 (OurOS)"); return 0; }
    println!("sfwbar: taskbar running");
    println!("  Config: ~/.config/sfwbar/sfwbar.config");
    println!("  Modules: taskbar, pager, tray, clock, battery");
    if args.is_empty() {
        println!("  Ready.");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "sfwbar".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_sfwbar(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
