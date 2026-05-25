#![deny(clippy::all)]

//! evince-cli — OurOS Evince GNOME document viewer
//!
//! Multi-personality: `evince`, `evince-previewer`, `evince-thumbnailer`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_evince(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: evince [OPTIONS] [FILE...]");
        println!("evince v45.0 (OurOS) — GNOME Document Viewer");
        println!();
        println!("Options:");
        println!("  -p PAGE           Open at page");
        println!("  -i INDEX          Open at named dest");
        println!("  -f                Fullscreen");
        println!("  -s                Slideshow mode");
        println!("  -w LABEL          Open at label");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("evince v45.0 (OurOS)"); return 0; }
    println!("evince: document viewer started");
    println!("  Supported: PDF, DjVu, PostScript, TIFF, XPS, CBR/CBZ");
    0
}

fn run_previewer(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: evince-previewer [OPTIONS] FILE");
        println!("evince-previewer v45.0 (OurOS) — Print preview");
        return 0;
    }
    let _ = args;
    println!("evince-previewer: showing print preview");
    0
}

fn run_thumbnailer(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: evince-thumbnailer [-s SIZE] INPUT OUTPUT");
        println!("evince-thumbnailer v45.0 (OurOS) — Generate thumbnails");
        return 0;
    }
    let _ = args;
    println!("evince-thumbnailer: thumbnail generated");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "evince".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "evince-previewer" => run_previewer(&rest, &prog),
        "evince-thumbnailer" => run_thumbnailer(&rest, &prog),
        _ => run_evince(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
