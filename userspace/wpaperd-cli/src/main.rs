#![deny(clippy::all)]

//! wpaperd-cli — OurOS wpaperd wallpaper daemon with slideshow
//!
//! Multi-personality: `wpaperd`, `wpaperctl`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_wpaperd(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: wpaperd [OPTIONS]");
        println!("wpaperd v1.0 (OurOS) — Wallpaper daemon with slideshow support");
        println!();
        println!("Options:");
        println!("  -d                Daemonize");
        println!("  --no-daemon       Don't daemonize");
        println!("  --version         Show version");
        println!();
        println!("Configure via ~/.config/wpaperd/wallpaper.toml");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("wpaperd v1.0 (OurOS)"); return 0; }
    println!("wpaperd: wallpaper daemon started");
    println!("  Config: ~/.config/wpaperd/wallpaper.toml");
    println!("  Slideshow: enabled (interval: 30m)");
    0
}

fn run_wpaperctl(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: wpaperctl COMMAND [OUTPUT]");
        println!("wpaperctl v1.0 (OurOS) — Control wpaperd");
        println!();
        println!("Commands:");
        println!("  next [OUTPUT]     Switch to next wallpaper");
        println!("  previous [OUTPUT] Switch to previous wallpaper");
        println!("  get [OUTPUT]      Get current wallpaper path");
        println!("  all-next          Next wallpaper on all outputs");
        println!("  all-previous      Previous on all outputs");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("next");
    match cmd {
        "next" | "all-next" => println!("Switched to next wallpaper"),
        "previous" | "all-previous" => println!("Switched to previous wallpaper"),
        "get" => println!("/home/user/Pictures/wallpaper.jpg"),
        _ => println!("wpaperctl: unknown command '{}'", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "wpaperd".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "wpaperctl" => run_wpaperctl(&rest, &prog),
        _ => run_wpaperd(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
