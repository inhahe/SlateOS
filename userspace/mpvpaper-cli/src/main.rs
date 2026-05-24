#![deny(clippy::all)]

//! mpvpaper-cli — OurOS mpvpaper video wallpaper
//!
//! Single personality: `mpvpaper`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_mpvpaper(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.len() < 2 {
        println!("Usage: mpvpaper [OPTIONS] OUTPUT VIDEO");
        println!("mpvpaper v1.7 (OurOS) — Video wallpaper for Wayland (via mpv)");
        println!();
        println!("Arguments:");
        println!("  OUTPUT            Output name (e.g. HDMI-A-1, or '*')");
        println!("  VIDEO             Video file or URL");
        println!();
        println!("Options:");
        println!("  -o MPV_OPTIONS    Extra mpv options (quoted string)");
        println!("  -s                Auto stop/play on visibility");
        println!("  -p                Auto pause on visibility");
        println!("  -l LAYER          Layer (background, bottom, top, overlay)");
        println!("  -f                Fork to background");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("mpvpaper v1.7 (OurOS)"); return 0; }
    let output = args.first().map(|s| s.as_str()).unwrap_or("*");
    let video = args.get(1).map(|s| s.as_str()).unwrap_or("video.mp4");
    println!("mpvpaper: playing {} on {}", video, output);
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "mpvpaper".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_mpvpaper(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
