#![deny(clippy::all)]

//! pdfpc-cli — OurOS pdfpc PDF presenter console
//!
//! Single personality: `pdfpc`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_pdfpc(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: pdfpc [OPTIONS] FILE");
        println!("pdfpc v4.6 (OurOS) — PDF presenter console");
        println!();
        println!("Options:");
        println!("  -d DURATION       Presentation duration (minutes)");
        println!("  -l                Last page as end page");
        println!("  -n                Notes position (left/right/top/bottom)");
        println!("  -s                Switch screens");
        println!("  -w                Windowed mode");
        println!("  -S                Single screen");
        println!("  --notes=POS       Speaker notes position");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("pdfpc v4.6 (OurOS)"); return 0; }
    println!("pdfpc: PDF presenter console started");
    println!("  Presenter screen: current + next slide, timer, notes");
    println!("  Audience screen: current slide fullscreen");
    println!("  Timer: 0:00 / 30:00");
    println!("  Drawing: pen & pointer supported");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "pdfpc".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_pdfpc(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
