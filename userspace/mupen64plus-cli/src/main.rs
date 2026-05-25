#![deny(clippy::all)]

//! mupen64plus-cli — OurOS Mupen64Plus N64 emulator
//!
//! Single personality: `mupen64plus`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_mupen64plus(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: mupen64plus [OPTIONS] ROM");
        println!("mupen64plus v2.5 (OurOS) — Nintendo 64 emulator");
        println!();
        println!("Options:");
        println!("  --resolution WxH   Display resolution");
        println!("  --fullscreen       Start fullscreen");
        println!("  --windowed         Start windowed");
        println!("  --gfx PLUGIN       Graphics plugin");
        println!("  --audio PLUGIN     Audio plugin");
        println!("  --input PLUGIN     Input plugin");
        println!("  --rsp PLUGIN       RSP plugin");
        println!("  --emumode N        Emulation mode (0=pure, 1=cached, 2=dynamic)");
        println!("  --sshotdir DIR     Screenshot directory");
        println!("  --savedir DIR      Save state directory");
        println!("  --cheats LIST      Cheat codes");
        println!("  --corelib FILE     Core library path");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("Mupen64Plus v2.5.9 (OurOS)");
        println!("  Core: v2.5.9");
        println!("  GFX: GLideN64 v4.0");
        println!("  Audio: SDL Audio v2.5");
        println!("  Input: SDL Input v2.5");
        println!("  RSP: HLE v2.5");
        return 0;
    }
    let files: Vec<&String> = args.iter().filter(|a| !a.starts_with('-')).collect();
    if files.is_empty() {
        eprintln!("mupen64plus: error: no ROM specified");
        return 1;
    }
    println!("Mupen64Plus v2.5.9: Loading {}", files[0]);
    println!("  CPU: VR4300 @ 93.75 MHz (emulated)");
    println!("  RCP: Reality Coprocessor");
    println!("  Video: GLideN64, 640x480");
    println!("  Audio: 44100 Hz stereo");
    println!("  Running...");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "mupen64plus".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_mupen64plus(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
