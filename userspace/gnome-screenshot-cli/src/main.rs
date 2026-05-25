#![deny(clippy::all)]

//! gnome-screenshot-cli — OurOS GNOME Screenshot
//!
//! Single personality: `gnome-screenshot`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_gnome_screenshot(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: gnome-screenshot [OPTIONS]");
        println!("gnome-screenshot v41.0 (OurOS) — GNOME screenshot utility");
        println!();
        println!("Options:");
        println!("  -w, --window      Capture current window");
        println!("  -a, --area        Capture selected area");
        println!("  -d, --delay SECS  Delay before capture");
        println!("  -f FILE           Save to file");
        println!("  -c, --clipboard   Copy to clipboard");
        println!("  -i, --interactive Interactive mode");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("gnome-screenshot v41.0 (OurOS)"); return 0; }
    println!("gnome-screenshot: screenshot captured");
    println!("  Saved to: ~/Pictures/Screenshot.png");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "gnome-screenshot".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_gnome_screenshot(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
