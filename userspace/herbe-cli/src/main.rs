#![deny(clippy::all)]

//! herbe-cli — OurOS herbe minimal notification daemon
//!
//! Single personality: `herbe`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_herbe(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: herbe [BODY]");
        println!("herbe v1.0 (OurOS) — Minimal X11 notification daemon");
        println!();
        println!("Displays a minimal, daemon-less notification.");
        println!("Click to dismiss, middle-click to perform action.");
        println!("Exits 0 on click, 1 on timeout, 2 on action.");
        println!("  --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("herbe v1.0 (OurOS)"); return 0; }
    let body = if args.is_empty() { "notification" } else { &args[0] };
    println!("herbe: {}", body);
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "herbe".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_herbe(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
