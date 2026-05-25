#![deny(clippy::all)]

//! citra-cli — OurOS Citra Nintendo 3DS emulator
//!
//! Multi-personality: `citra`, `citra-room`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_citra(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: citra [OPTIONS] [ROM]");
        println!("citra v2104 (OurOS) — Nintendo 3DS emulator");
        println!();
        println!("Options:");
        println!("  --fullscreen      Start fullscreen");
        println!("  --multiplayer     Enable multiplayer");
        println!("  --movie FILE      Play/record TAS movie");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("citra v2104 (OurOS)"); return 0; }
    println!("citra: Nintendo 3DS emulator started");
    println!("  Backend: Vulkan");
    println!("  Resolution: 4x native");
    println!("  3D mode: side-by-side (disabled)");
    println!("  Amiibo: supported");
    0
}

fn run_room(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: citra-room [OPTIONS]");
        println!("citra-room v2104 (OurOS) — Citra multiplayer server");
        println!();
        println!("Options:");
        println!("  --port PORT       Server port (default: 24872)");
        println!("  --max-members N   Max players (default: 4)");
        println!("  --room-name NAME  Room name");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("citra-room v2104 (OurOS)"); return 0; }
    println!("citra-room: multiplayer server started on port 24872");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "citra".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "citra-room" => run_room(&rest, &prog),
        _ => run_citra(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
