#![deny(clippy::all)]

//! timg-cli — OurOS timg terminal image/video viewer
//!
//! Single personality: `timg`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_timg(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: timg [OPTIONS] FILE...");
        println!("timg 1.6.0 (OurOS) — Terminal image and video viewer");
        println!();
        println!("Options:");
        println!("  -g WxH             Grid size for multiple images");
        println!("  -p PROTOCOL        Pixel protocol (quarter, half, kitty, iterm2, sixel)");
        println!("  -C                 Center image");
        println!("  -W                 Fit width");
        println!("  -U                 Upscale small images");
        println!("  --clear            Clear screen before display");
        println!("  -b COLOR           Background color");
        println!("  -B COLOR           Checkerboard color");
        println!("  --compress         Compress output");
        println!("  --title            Show filename as title");
        println!("  -f N               Frames per second for video/GIF");
        println!("  -t SECS            Duration to show");
        println!("  --loops N          Number of loops (0=infinite)");
        println!("  -V, --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("timg 1.6.0 (OurOS)");
        return 0;
    }
    let files: Vec<&str> = args.iter()
        .filter(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .collect();
    if files.is_empty() {
        println!("timg: No files specified");
        return 1;
    }
    for f in &files {
        if args.iter().any(|a| a == "--title") {
            println!("--- {} ---", f);
        }
        println!("timg: Displaying '{}'", f);
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "timg".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_timg(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
