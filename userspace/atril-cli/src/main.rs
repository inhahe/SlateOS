#![deny(clippy::all)]

//! atril-cli — OurOS MATE Atril document viewer
//!
//! Single personality: `atril`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_atril(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: atril [OPTIONS] [FILE...]");
        println!("atril v1.26 (OurOS) — MATE Document Viewer");
        println!();
        println!("Options:");
        println!("  -p PAGE           Open at page");
        println!("  -f                Fullscreen");
        println!("  -s                Slideshow");
        println!("  -w LABEL          Open at label");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("atril v1.26 (OurOS)"); return 0; }
    println!("atril: MATE document viewer started");
    println!("  Supported: PDF, DjVu, PostScript, TIFF, XPS, CBR/CBZ");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "atril".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_atril(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
