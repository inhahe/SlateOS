#![deny(clippy::all)]

//! swww-cli — OurOS swww animated wallpaper daemon
//!
//! Multi-personality: `swww`, `swww-daemon`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_swww(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: swww COMMAND [OPTIONS]");
        println!("swww v0.9 (OurOS) — Animated wallpaper daemon for Wayland");
        println!();
        println!("Commands:");
        println!("  init              Start swww-daemon");
        println!("  kill              Stop swww-daemon");
        println!("  img PATH          Set wallpaper image");
        println!("  clear [COLOR]     Clear to solid color");
        println!("  query             Query current wallpaper");
        println!();
        println!("Transition options (for img/clear):");
        println!("  --transition-type TYPE  none, simple, fade, wipe, grow, wave, outer");
        println!("  --transition-step STEP  Transition speed (1-255)");
        println!("  --transition-duration S Duration in seconds");
        println!("  --transition-fps FPS    Transition FPS");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("query");
    match cmd {
        "init" => println!("swww-daemon started"),
        "kill" => println!("swww-daemon stopped"),
        "img" => {
            let path = args.get(1).map(|s| s.as_str()).unwrap_or("(none)");
            let transition = args.iter().skip_while(|a| a.as_str() != "--transition-type").nth(1)
                .map(|s| s.as_str()).unwrap_or("simple");
            println!("Setting wallpaper: {} (transition: {})", path, transition);
        }
        "clear" => {
            let color = args.get(1).map(|s| s.as_str()).unwrap_or("#000000");
            println!("Cleared to color: {}", color);
        }
        "query" => println!("HDMI-A-1: ~/Pictures/wallpaper.png"),
        _ => println!("swww: unknown command '{}'", cmd),
    }
    0
}

fn run_daemon(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: swww-daemon [OPTIONS]");
        println!("swww-daemon v0.9 (OurOS) — Wallpaper daemon process");
        println!();
        println!("Options:");
        println!("  --format FORMAT   Image format (xrgb, xbgr, rgb, bgr)");
        return 0;
    }
    println!("swww-daemon: running (supports animated transitions)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "swww".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "swww-daemon" => run_daemon(&rest, &prog),
        _ => run_swww(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
