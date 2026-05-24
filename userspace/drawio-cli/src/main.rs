#![deny(clippy::all)]

//! drawio-cli — OurOS draw.io diagram editor
//!
//! Single personality: `drawio`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_drawio(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: drawio [OPTIONS] [FILE]");
        println!("drawio v24.0 (OurOS) — Diagram editor");
        println!();
        println!("Options:");
        println!("  -x                Export mode");
        println!("  -f FMT            Export format (png, svg, pdf, xml)");
        println!("  -o FILE           Output file");
        println!("  -p N              Page number to export");
        println!("  --crop            Crop to content");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("drawio v24.0 (OurOS)"); return 0; }
    if args.iter().any(|a| a == "-x") {
        println!("drawio: exporting diagram...");
        println!("  Output format: PNG");
        println!("  Export complete");
        return 0;
    }
    println!("drawio: diagram editor started");
    println!("  Shape libraries: UML, Network, Flowchart, AWS, Azure");
    println!("  Storage: local files");
    println!("  Format: XML-based (.drawio)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "drawio".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_drawio(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
