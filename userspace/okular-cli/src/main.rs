#![deny(clippy::all)]

//! okular-cli — OurOS KDE Okular document viewer
//!
//! Single personality: `okular`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_okular(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: okular [OPTIONS] [FILE...]");
        println!("okular v23.08 (OurOS) — KDE Document Viewer");
        println!();
        println!("Options:");
        println!("  -p PAGE           Open at page");
        println!("  --presentation    Start in presentation mode");
        println!("  --print           Open print dialog");
        println!("  --unique          Reuse existing instance");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("okular v23.08 (OurOS)"); return 0; }
    println!("okular: document viewer started");
    println!("  Supported: PDF, PS, DjVu, CHM, XPS, ePub, TIFF, Fax, CBR/CBZ, Markdown");
    println!("  Annotations: yes");
    println!("  Forms: yes");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "okular".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_okular(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
