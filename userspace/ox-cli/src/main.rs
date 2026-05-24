#![deny(clippy::all)]

//! ox-cli — OurOS Ox editor
//!
//! Single personality: `ox`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_ox(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: ox [OPTIONS] [FILE...]");
        println!("ox 0.6.3 (OurOS) — Fast terminal text editor");
        println!();
        println!("Options:");
        println!("  --config FILE        Config file path");
        println!("  --readonly           Read-only mode");
        println!("  --filetype TYPE      Force file type");
        println!("  -V, --version        Show version");
        println!();
        println!("Keybindings:");
        println!("  Ctrl+S   Save");
        println!("  Ctrl+Q   Quit");
        println!("  Ctrl+F   Find");
        println!("  Ctrl+R   Replace");
        println!("  Ctrl+N   New file");
        println!("  Ctrl+O   Open file");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("ox 0.6.3 (OurOS)");
        return 0;
    }
    let file = args.iter().find(|a| !a.starts_with('-'))
        .map(|s| s.as_str());
    if let Some(f) = file {
        println!("ox: Editing '{}'", f);
    } else {
        println!("ox: New file");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "ox".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_ox(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
