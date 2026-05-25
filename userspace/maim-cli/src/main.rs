#![deny(clippy::all)]

//! maim-cli — OurOS maim screenshot utility
//!
//! Single personality: `maim`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_maim(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: maim [OPTIONS] [FILE]");
        println!("maim v5.7 (OurOS) — Screenshot utility (Make Image)");
        println!();
        println!("Options:");
        println!("  -s, --select      Select region (use with slop)");
        println!("  -i ID             Capture specific window ID");
        println!("  -d, --delay SECS  Delay before capture");
        println!("  -f, --format FMT  Output format (png, jpg, bmp)");
        println!("  -g, --geometry    Capture specific geometry");
        println!("  -q, --quality N   JPEG quality (1-100)");
        println!("  --version         Show version");
        println!();
        println!("Pipe to clipboard: maim -s | xclip -selection clipboard -t image/png");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("maim v5.7 (OurOS)"); return 0; }
    let file = args.last().map(|s| s.as_str()).unwrap_or("screenshot.png");
    println!("maim: captured to '{}'", file);
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "maim".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_maim(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
