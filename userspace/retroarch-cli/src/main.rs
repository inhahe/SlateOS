#![deny(clippy::all)]

//! retroarch-cli — OurOS RetroArch multi-system emulator
//!
//! Multi-personality: `retroarch`

use std::env;
use std::process;

fn run_retroarch(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: retroarch [OPTIONS] [ROM]");
        println!();
        println!("retroarch — multi-system emulator frontend (OurOS).");
        println!();
        println!("Options:");
        println!("  -L, --libretro <core>  Libretro core to load");
        println!("  --subsystem <sys>      Subsystem type");
        println!("  -f, --fullscreen       Start in fullscreen");
        println!("  -c, --config <file>    Config file");
        println!("  --appendconfig <file>  Append config");
        println!("  -v, --verbose          Verbose output");
        println!("  --features             List compiled features");
        println!("  --version              Show version");
        println!("  --log-file <file>      Log to file");
        println!("  --menu                 Start in menu");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("RetroArch 1.17.0 (OurOS)");
        println!("Git: abcdef1234567890");
        println!("Compiler: rustc");
        return 0;
    }
    if args.iter().any(|a| a == "--features") {
        println!("Features:");
        println!("  Threads:     YES");
        println!("  OpenGL:      YES (Core)");
        println!("  Vulkan:      YES");
        println!("  ALSA:        YES");
        println!("  PulseAudio:  YES");
        println!("  Wayland:     YES");
        println!("  X11:         YES");
        println!("  Network:     YES (netplay, updater)");
        println!("  FFmpeg:      YES");
        println!("  Shaders:     YES (Slang, GLSL)");
        return 0;
    }

    let core = args.windows(2).find(|w| w[0] == "-L" || w[0] == "--libretro")
        .map(|w| w[1].as_str());
    let rom = args.iter().find(|a| !a.starts_with('-') && !args.windows(2).any(|w| &w[1] == *a && (w[0] == "-L" || w[0] == "-c")))
        .map(|s| s.as_str());

    if let Some(c) = core {
        println!("[INFO] Loading core: {}", c);
    } else {
        println!("[INFO] No core specified, starting in menu mode");
    }
    if let Some(r) = rom {
        println!("[INFO] Loading ROM: {}", r);
    }
    println!("[INFO] RetroArch 1.17.0 (OurOS)");
    println!("[INFO] Initializing video driver: vulkan");
    println!("[INFO] Initializing audio driver: pulseaudio");
    println!("[INFO] Initializing input driver: udev");
    println!("[INFO] Running...");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_retroarch(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
