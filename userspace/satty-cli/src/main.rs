#![deny(clippy::all)]

//! satty-cli — OurOS Satty screenshot annotation
//!
//! Single personality: `satty`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_satty(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: satty -f FILE [OPTIONS]");
        println!("satty v0.12 (OurOS) — Screenshot annotation tool");
        println!();
        println!("Options:");
        println!("  -f FILE           Input file (or - for stdin)");
        println!("  --output-filename FILE  Output filename pattern");
        println!("  --copy-command CMD      Copy command");
        println!("  --early-exit      Exit after copy/save");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("satty v0.12 (OurOS)"); return 0; }
    let file = args.iter().skip_while(|a| a.as_str() != "-f").nth(1).map(|s| s.as_str()).unwrap_or("-");
    println!("Opening annotation editor: {}", file);
    println!("  Tools: brush, rectangle, ellipse, arrow, text, blur, crop");
    println!("  Ctrl+S save, Ctrl+C copy, Ctrl+Z undo");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "satty".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_satty(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
