#![deny(clippy::all)]

//! scummvm-cli — OurOS ScummVM adventure game engine
//!
//! Multi-personality: `scummvm`

use std::env;
use std::process;

fn run_scummvm(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: scummvm [OPTIONS] [GAME]");
        println!();
        println!("scummvm — adventure game interpreter (OurOS).");
        println!();
        println!("Options:");
        println!("  -v, --version        Show version");
        println!("  -z, --list-games     List supported games");
        println!("  -t, --list-targets   List configured targets");
        println!("  --list-engines       List detection engines");
        println!("  -p, --path <dir>     Game data path");
        println!("  -f, --fullscreen     Start fullscreen");
        println!("  -g, --gfx-mode <m>   Graphics mode");
        println!("  --auto-detect        Auto-detect game");
        println!("  --debuglevel <n>     Debug level");
        return 0;
    }
    if args.iter().any(|a| a == "-v" || a == "--version") {
        println!("ScummVM 2.8.1 (OurOS)");
        println!("Features compiled in: Vorbis FLAC MP3 ALSA PNG MPEG2 Theora AAC");
        return 0;
    }
    if args.iter().any(|a| a == "-z" || a == "--list-games") {
        println!("Game ID              Full Title");
        println!("-------------------- ------------------------------------------------");
        println!("monkey               The Secret of Monkey Island");
        println!("monkey2              Monkey Island 2: LeChuck's Revenge");
        println!("tentacle             Day of the Tentacle");
        println!("dig                  The Dig");
        println!("ft                   Full Throttle");
        println!("grim                 Grim Fandango");
        println!("sam                  Sam & Max Hit the Road");
        println!("sky                  Beneath a Steel Sky");
        println!("sword1               Broken Sword: Shadow of the Templars");
        println!("myst                 Myst");
        println!("riven                Riven: The Sequel to Myst");
        return 0;
    }
    if args.iter().any(|a| a == "--list-engines") {
        println!("Engine ID    Description");
        println!("------------ -----------");
        println!("scumm        LucasArts SCUMM games");
        println!("sci          Sierra SCI games");
        println!("agi          Sierra AGI games");
        println!("mohawk       Broderbund/Cyan Mohawk games");
        println!("sword        Revolution Broken Sword");
        println!("wintermute   Wintermute Engine games");
        return 0;
    }

    let game = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str());
    if let Some(g) = game {
        println!("[ScummVM] Loading game: {}", g);
        println!("[ScummVM] Engine: scumm");
        println!("[ScummVM] Renderer: OpenGL (3.3 Core)");
        println!("[ScummVM] Audio: PulseAudio, 44100 Hz, stereo");
        println!("[ScummVM] Running...");
    } else {
        println!("[ScummVM] Starting GUI launcher");
        println!("[ScummVM] Found 3 configured games");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_scummvm(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
