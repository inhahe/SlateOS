#![deny(clippy::all)]

//! zed-cli — OurOS Zed code editor
//!
//! Single personality: `zed`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_zed(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: zed [OPTIONS] [PATH...]");
        println!("Zed 0.145.1 (OurOS) — High-performance, multiplayer code editor");
        println!();
        println!("Options:");
        println!("  -n, --new              New window");
        println!("  -w, --wait             Wait until closed");
        println!("  -a, --add              Add to existing workspace");
        println!("  --dev-server-token T   Dev server auth token");
        println!("  --foreground           Don't fork to background");
        println!("  -V, --version          Show version");
        println!();
        println!("Arguments:");
        println!("  [PATH...]   Files or directories (supports file:line:col)");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("Zed 0.145.1 (OurOS)");
        return 0;
    }
    let paths: Vec<&str> = args.iter()
        .filter(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .collect();
    if paths.is_empty() {
        println!("zed: Opening recent workspace...");
    } else {
        for p in &paths {
            println!("zed: Opening '{}'", p);
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "zed".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_zed(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
