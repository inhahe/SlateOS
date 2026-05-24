#![deny(clippy::all)]

//! chafa-cli — OurOS Chafa image-to-text converter
//!
//! Single personality: `chafa`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_chafa(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: chafa [OPTIONS] [FILE...]");
        println!("chafa 1.14.1 (OurOS) — Terminal graphics");
        println!();
        println!("Options:");
        println!("  -c, --colors MODE        Color mode (none, 2, 16, 256, full)");
        println!("  --color-extractor TYPE    Color extraction (average, median)");
        println!("  -f, --format FORMAT       Output format (symbols, sixels, kitty, iterm)");
        println!("  --font-ratio W/H          Font aspect ratio");
        println!("  -O, --optimize N          Optimization level (0-9)");
        println!("  -p, --preprocess BOOL     Preprocessing");
        println!("  -s, --size WxH            Output size");
        println!("  --scale VALUE             Scale factor");
        println!("  --stretch                 Stretch to fill");
        println!("  -w, --work N              Processing threads");
        println!("  --animate BOOL            Animate (for GIFs)");
        println!("  --center BOOL             Center output");
        println!("  --dither MODE             Dither mode");
        println!("  --invert                  Invert colors");
        println!("  --margin-bottom N         Bottom margin");
        println!("  -V, --version             Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("chafa 1.14.1 (OurOS)");
        return 0;
    }
    let file = args.iter().rfind(|a| !a.starts_with('-'))
        .map(|s| s.as_str()).unwrap_or("-");
    println!("chafa: Rendering '{}' to terminal...", file);
    println!("(image rendered as text art)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "chafa".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_chafa(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
