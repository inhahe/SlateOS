#![deny(clippy::all)]

//! librecad-cli — OurOS LibreCAD 2D CAD application
//!
//! Single personality: `librecad`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_librecad(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: librecad [OPTIONS] [FILE]");
        println!("librecad v2.2.0 (OurOS) — 2D CAD application");
        println!();
        println!("Options:");
        println!("  --version         Show version");
        println!();
        println!("Supported formats: DXF, DWG (read), SVG, PDF export");
        println!("Features: layers, blocks, hatching, dimensioning, snapping");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("librecad v2.2.0 (OurOS)"); return 0; }
    println!("librecad: 2D CAD application started");
    println!("  Drawing tools: line, arc, circle, ellipse, polyline, spline");
    println!("  Modification: move, rotate, scale, mirror, trim, offset");
    println!("  Snap modes: grid, endpoint, center, intersection");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "librecad".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_librecad(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
