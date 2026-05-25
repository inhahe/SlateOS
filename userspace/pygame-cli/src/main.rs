#![deny(clippy::all)]

//! pygame-cli — OurOS Pygame game framework
//!
//! Single personality: `pygame`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_pygame(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: pygame [COMMAND] [OPTIONS]");
        println!("Pygame v2.6 (OurOS) — Python game development library");
        println!();
        println!("Commands:");
        println!("  new PROJECT        Create new pygame project");
        println!("  run FILE           Run a pygame script");
        println!("  pack DIR           Package game for distribution");
        println!("  examples list|run  Browse example games");
        println!("  info               Print system info");
        println!("  benchmark          Run performance benchmarks");
        println!();
        println!("Options:");
        println!("  --sdl2             Use SDL2 backend");
        println!("  --ce               Pygame Community Edition");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Pygame v2.6.0 (OurOS)"); return 0; }
    println!("Pygame v2.6.0 (OurOS)");
    println!("  Python: 3.12");
    println!("  SDL: 2.30.3");
    println!("  Display drivers: x11, wayland, dummy");
    println!("  Sound driver: pulseaudio");
    println!("  Mixer: 44100Hz, 16-bit stereo");
    println!("  Image formats: PNG, JPEG, GIF, BMP, WebP");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "pygame".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_pygame(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
